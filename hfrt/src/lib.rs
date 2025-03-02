use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer, MintTo};

declare_id!("A86NRtxqJiyKm4da9jmA1TH1erjUG3ULcPXhS6wdyQk7");

/// A constant for the staking vault seed with a `'static` lifetime.
const VAULT_SEED: &[u8] = b"staking-vault";
/// Module-level seeds for the staking vault PDA.
const STAKING_VAULT_SEEDS: &[&[u8]] = &[VAULT_SEED];
/// Module-level signer seeds array for the staking vault PDA.
const STAKING_VAULT_SIGNER: &[&[&[u8]]] = &[STAKING_VAULT_SEEDS];

#[program]
pub mod hfrt {
    use super::*;

    /// Initializes the global state and sets up the HFRT mint.
    pub fn initialize(ctx: Context<Initialize>, fee_discount: u8) -> Result<()> {
        let state = &mut ctx.accounts.global_state;
        state.authority = ctx.accounts.authority.key();
        state.hfrt_mint = ctx.accounts.hfrt_mint.key();
        state.fee_discount = fee_discount;
        state.bump = ctx.bumps.global_state; // Retrieve bump via dot notation.
        Ok(())
    }

    /// Initializes the Governance account for managing rebate parameters.
    pub fn initialize_governance(
        ctx: Context<InitializeGovernance>,
        rebate_rate: u8,
        max_fee_discount: u8,
    ) -> Result<()> {
        let gov = &mut ctx.accounts.governance;
        gov.rebate_rate = rebate_rate;
        gov.max_fee_discount = max_fee_discount;
        gov.authority = ctx.accounts.authority.key();
        Ok(())
    }

    /// Updates the rebate rate via governance.
    pub fn update_rebate_rate(ctx: Context<UpdateGovernance>, new_rate: u8) -> Result<()> {
        let gov = &mut ctx.accounts.governance;
        require!(new_rate <= gov.max_fee_discount, ErrorCode::InvalidRebateRate);
        gov.rebate_rate = new_rate;
        Ok(())
    }

    /// Records a trade by updating the trader’s 24-hour rolling volume.
    /// Resets the volume if more than 24 hours have elapsed.
    /// Checks for wash trades and for too-frequent trading (sybil resistance).
    pub fn record_trade(ctx: Context<RecordTrade>, trade_amount: u64) -> Result<()> {
        let trader = &mut ctx.accounts.trader;
        let clock = Clock::get()?;
        let current_time = clock.unix_timestamp;

        // Check for potential wash trading.
        if is_wash_trade(trader.last_update, current_time, trade_amount) {
            return Err(ErrorCode::WashTrade.into());
        }
        // Check if trades occur too frequently.
        if detect_frequent_trades(trader.last_update, current_time) {
            return Err(ErrorCode::FrequentTrades.into());
        }

        if current_time - trader.last_update >= 24 * 3600 {
            trader.rolling_volume = trade_amount;
        } else {
            trader.rolling_volume = trader
                .rolling_volume
                .checked_add(trade_amount)
                .ok_or(ErrorCode::Overflow)?;
        }
        trader.last_update = current_time;

        emit!(TradeRecorded {
            owner: trader.owner,
            trade_amount,
            rolling_volume: trader.rolling_volume,
        });
        Ok(())
    }

    /// Claims an HFRT rebate based on the recorded 24-hour trading volume.
    /// The rebate is computed using the governance rebate rate and a multiplier.
    pub fn claim_rebate(ctx: Context<ClaimRebate>) -> Result<()> {
        let rebate_amount = {
            let trader = &mut ctx.accounts.trader;
            let base_rebate = trader
                .rolling_volume
                .checked_mul(ctx.accounts.governance.rebate_rate as u64)
                .ok_or(ErrorCode::Overflow)?
                .checked_div(1000)
                .ok_or(ErrorCode::Overflow)?;
            // Apply a multiplier based on volume.
            let multiplier = calculate_rebate_multiplier(trader.rolling_volume);
            let total_rebate = base_rebate.checked_mul(multiplier as u64).ok_or(ErrorCode::Overflow)?;
            trader.rolling_volume = 0;
            total_rebate
        };
        token::mint_to(ctx.accounts.into_mint_to_context(), rebate_amount)?;

        let owner = ctx.accounts.trader.owner;
        emit!(RebateClaimed { owner, rebate_amount });
        Ok(())
    }

    /// Stakes HFRT tokens by transferring them from the trader’s token account into the staking vault.
    /// Records the stake start time if this is the first stake.
    pub fn stake_tokens(ctx: Context<StakeTokens>, amount: u64) -> Result<()> {
        token::transfer(ctx.accounts.into_transfer_to_vault_context(), amount)?;
        {
            let trader = &mut ctx.accounts.trader;
            trader.staked_amount = trader
                .staked_amount
                .checked_add(amount)
                .ok_or(ErrorCode::Overflow)?;
            if trader.stake_start_time == 0 {
                let clock = Clock::get()?;
                trader.stake_start_time = clock.unix_timestamp;
            }
        }
        Ok(())
    }

    /// Unstakes HFRT tokens by transferring them back from the staking vault.
    /// Applies a dynamic unstake penalty based on staking duration.
    pub fn unstake_tokens(ctx: Context<UnstakeTokens>, amount: u64) -> Result<()> {
        let amount_after_penalty = {
            let trader = &mut ctx.accounts.trader;
            require!(trader.staked_amount >= amount, ErrorCode::InsufficientStake);
            let clock = Clock::get()?;
            let penalty = calculate_dynamic_unstake_penalty(trader.stake_start_time, clock.unix_timestamp, amount);
            let amount_after_penalty = amount.checked_sub(penalty).ok_or(ErrorCode::Overflow)?;
            trader.staked_amount = trader
                .staked_amount
                .checked_sub(amount)
                .ok_or(ErrorCode::Overflow)?;
            if trader.staked_amount == 0 {
                trader.stake_start_time = 0;
            }
            amount_after_penalty
        };
        token::transfer(ctx.accounts.into_transfer_from_vault_context(), amount_after_penalty)?;
        Ok(())
    }

    /// Auto-compounds staking rewards by minting the rebate directly to the staking vault.
    pub fn auto_compound(ctx: Context<AutoCompound>) -> Result<()> {
        let rebate_amount = {
            let trader = &mut ctx.accounts.trader;
            let base_rebate = trader
                .rolling_volume
                .checked_mul(ctx.accounts.governance.rebate_rate as u64)
                .ok_or(ErrorCode::Overflow)?
                .checked_div(1000)
                .ok_or(ErrorCode::Overflow)?;
            let multiplier = calculate_rebate_multiplier(trader.rolling_volume);
            let total_rebate = base_rebate.checked_mul(multiplier as u64).ok_or(ErrorCode::Overflow)?;
            trader.rolling_volume = 0;
            total_rebate
        };
        token::mint_to(ctx.accounts.into_mint_to_vault_context(), rebate_amount)?;
        {
            let trader = &mut ctx.accounts.trader;
            trader.staked_amount = trader.staked_amount.checked_add(rebate_amount).ok_or(ErrorCode::Overflow)?;
            if trader.stake_start_time == 0 {
                let clock = Clock::get()?;
                trader.stake_start_time = clock.unix_timestamp;
            }
        }
        Ok(())
    }

    /// Creates a new DAO proposal to update the fee discount.
    pub fn create_dao_proposal(
        ctx: Context<CreateDAOProposal>,
        proposal_id: u64,
        new_fee_discount: u8,
    ) -> Result<()> {
        let proposal = &mut ctx.accounts.dao_proposal;
        proposal.proposal_id = proposal_id;
        proposal.proposer = ctx.accounts.proposer.key();
        proposal.new_fee_discount = new_fee_discount;
        proposal.votes_for = 0;
        proposal.votes_against = 0;
        Ok(())
    }

    /// Votes on an existing DAO proposal.
    pub fn vote_dao_proposal(ctx: Context<VoteDAOProposal>, vote_for: bool) -> Result<()> {
        let proposal = &mut ctx.accounts.dao_proposal;
        if vote_for {
            proposal.votes_for = proposal.votes_for.checked_add(1).ok_or(ErrorCode::Overflow)?;
        } else {
            proposal.votes_against = proposal.votes_against.checked_add(1).ok_or(ErrorCode::Overflow)?;
        }
        Ok(())
    }

    /// Executes a DAO proposal if it has passed, updating the fee discount.
    pub fn execute_dao_proposal(ctx: Context<ExecuteDAOProposal>) -> Result<()> {
        let proposal = &ctx.accounts.dao_proposal;
        require!(proposal.votes_for > proposal.votes_against, ErrorCode::ProposalRejected);
        let global_state = &mut ctx.accounts.global_state;
        global_state.fee_discount = proposal.new_fee_discount;
        Ok(())
    }
}

/// Returns a multiplier for the rebate based on the 24‑hour trading volume.
fn calculate_rebate_multiplier(trade_volume: u64) -> u8 {
    if trade_volume >= 100_000_000 {
        5
    } else if trade_volume >= 50_000_000 {
        3
    } else if trade_volume >= 10_000_000 {
        2
    } else {
        1
    }
}

/// Returns true if trades occur too frequently (less than 5 seconds apart).
fn detect_frequent_trades(last_trade_time: i64, current_time: i64) -> bool {
    let min_time_between_trades = 5;
    (current_time - last_trade_time) < min_time_between_trades
}

/// Returns true if a trade is considered a wash trade.
fn is_wash_trade(last_trade_time: i64, current_time: i64, trade_amount: u64) -> bool {
    let min_time_between_trades = 10;
    trade_amount > 1_000_000 && (current_time - last_trade_time) < min_time_between_trades
}

/// Calculates a dynamic unstake penalty based on staking duration.
/// Penalty: 10% if staked less than 7 days, 5% if less than 14 days, 2% otherwise.
fn calculate_dynamic_unstake_penalty(stake_start_time: i64, current_time: i64, amount: u64) -> u64 {
    let duration = current_time - stake_start_time;
    let penalty_percentage = if duration < 7 * 24 * 3600 {
        10
    } else if duration < 14 * 24 * 3600 {
        5
    } else {
        2
    };
    amount * penalty_percentage / 100
}

/// Calculates execution priority based on HFRT balance (lower number means higher priority).
fn calculate_execution_priority(hfrt_balance: u64) -> u8 {
    if hfrt_balance >= 1_000_000 {
        1
    } else if hfrt_balance >= 500_000 {
        2
    } else if hfrt_balance >= 100_000 {
        3
    } else {
        5
    }
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    /// Global state PDA (seeded by "global-state")
    #[account(
        init,
        payer = authority,
        seeds = [b"global-state"],
        bump,
        space = 8 + GlobalState::LEN,
    )]
    pub global_state: Account<'info, GlobalState>,
    #[account(mut)]
    pub authority: Signer<'info>,
    /// The mint for HFRT tokens.
    pub hfrt_mint: Account<'info, Mint>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct InitializeGovernance<'info> {
    #[account(
        init,
        payer = authority,
        space = 8 + Governance::LEN,
    )]
    pub governance: Account<'info, Governance>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct UpdateGovernance<'info> {
    #[account(mut, has_one = authority)]
    pub governance: Account<'info, Governance>,
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct RecordTrade<'info> {
    /// Trader state account (must be pre-initialized).
    #[account(mut, has_one = owner)]
    pub trader: Account<'info, Trader>,
    #[account(mut)]
    pub owner: Signer<'info>,
}

#[derive(Accounts)]
pub struct ClaimRebate<'info> {
    /// Trader state account (must be pre-initialized).
    #[account(mut, has_one = owner)]
    pub trader: Account<'info, Trader>,
    #[account(mut)]
    pub owner: Signer<'info>,
    /// The HFRT mint.
    #[account(mut)]
    pub hfrt_mint: Account<'info, Mint>,
    /// Trader’s token account for receiving rebates.
    #[account(mut)]
    pub trader_token_account: Account<'info, TokenAccount>,
    /// PDA mint authority (seeded by "mint-authority").
    #[account(
        seeds = [b"mint-authority"],
        bump,
    )]
    /// CHECK: This PDA is derived deterministically.
    pub mint_authority: UncheckedAccount<'info>,
    /// Governance account for rebate rate configuration.
    #[account(mut)]
    pub governance: Account<'info, Governance>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct StakeTokens<'info> {
    /// Trader state account (must be pre-initialized).
    #[account(mut, has_one = owner)]
    pub trader: Account<'info, Trader>,
    #[account(mut)]
    pub owner: Signer<'info>,
    /// Trader’s HFRT token account.
    #[account(mut)]
    pub trader_token_account: Account<'info, TokenAccount>,
    /// Staking vault PDA (seeded by "staking-vault").
    #[account(
        mut,
        seeds = [VAULT_SEED],
        bump,
    )]
    /// CHECK: This PDA holds staked tokens.
    pub staking_vault: UncheckedAccount<'info>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct UnstakeTokens<'info> {
    /// Trader state account (must be pre-initialized).
    #[account(mut, has_one = owner)]
    pub trader: Account<'info, Trader>,
    #[account(mut)]
    pub owner: Signer<'info>,
    /// Trader’s HFRT token account.
    #[account(mut)]
    pub trader_token_account: Account<'info, TokenAccount>,
    /// Staking vault PDA (seeded by "staking-vault").
    #[account(
        mut,
        seeds = [VAULT_SEED],
        bump,
    )]
    /// CHECK: This PDA holds staked tokens.
    pub staking_vault: UncheckedAccount<'info>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct AutoCompound<'info> {
    /// Trader state account (must be pre-initialized).
    #[account(mut, has_one = owner)]
    pub trader: Account<'info, Trader>,
    #[account(mut)]
    pub owner: Signer<'info>,
    /// The HFRT mint.
    #[account(mut)]
    pub hfrt_mint: Account<'info, Mint>,
    /// Staking vault PDA (seeded by "staking-vault").
    #[account(
        mut,
        seeds = [VAULT_SEED],
        bump,
    )]
    /// CHECK: This PDA holds staked tokens.
    pub staking_vault: UncheckedAccount<'info>,
    /// PDA mint authority (seeded by "mint-authority").
    #[account(
        seeds = [b"mint-authority"],
        bump,
    )]
    /// CHECK: This PDA is derived deterministically.
    pub mint_authority: UncheckedAccount<'info>,
    /// Governance account for rebate rate configuration.
    #[account(mut)]
    pub governance: Account<'info, Governance>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct CreateDAOProposal<'info> {
    #[account(
        init,
        payer = proposer,
        space = 8 + DAOProposal::LEN,
    )]
    pub dao_proposal: Account<'info, DAOProposal>,
    #[account(mut)]
    pub proposer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct VoteDAOProposal<'info> {
    #[account(mut)]
    pub dao_proposal: Account<'info, DAOProposal>,
    pub voter: Signer<'info>,
}

#[derive(Accounts)]
pub struct ExecuteDAOProposal<'info> {
    #[account(mut)]
    pub dao_proposal: Account<'info, DAOProposal>,
    #[account(mut)]
    pub global_state: Account<'info, GlobalState>,
    pub authority: Signer<'info>,
}

impl<'info> ClaimRebate<'info> {
    /// Prepares the context for minting tokens to the trader.
    fn into_mint_to_context(&self) -> CpiContext<'_, '_, '_, 'info, MintTo<'info>> {
        let cpi_accounts = MintTo {
            mint: self.hfrt_mint.to_account_info().clone(),
            to: self.trader_token_account.to_account_info().clone(),
            authority: self.mint_authority.to_account_info().clone(),
        };
        CpiContext::new(self.token_program.to_account_info().clone(), cpi_accounts)
    }
}

impl<'info> StakeTokens<'info> {
    /// Prepares the context for transferring tokens from the trader to the staking vault.
    fn into_transfer_to_vault_context(&self) -> CpiContext<'_, '_, '_, 'info, Transfer<'info>> {
        let cpi_accounts = Transfer {
            from: self.trader_token_account.to_account_info().clone(),
            to: self.staking_vault.to_account_info().clone(),
            authority: self.owner.to_account_info().clone(),
        };
        CpiContext::new(self.token_program.to_account_info().clone(), cpi_accounts)
    }
}

impl<'info> UnstakeTokens<'info> {
    /// Prepares the context for transferring tokens from the staking vault back to the trader.
    fn into_transfer_from_vault_context(&self) -> CpiContext<'_, '_, '_, 'info, Transfer<'info>> {
        let cpi_accounts = Transfer {
            from: self.staking_vault.to_account_info().clone(),
            to: self.trader_token_account.to_account_info().clone(),
            authority: self.staking_vault.to_account_info().clone(),
        };
        CpiContext::new_with_signer(
            self.token_program.to_account_info().clone(),
            cpi_accounts,
            STAKING_VAULT_SIGNER,
        )
    }
}

impl<'info> AutoCompound<'info> {
    /// Prepares the context for minting tokens directly to the staking vault.
    fn into_mint_to_vault_context(&self) -> CpiContext<'_, '_, '_, 'info, MintTo<'info>> {
        let cpi_accounts = MintTo {
            mint: self.hfrt_mint.to_account_info().clone(),
            to: self.staking_vault.to_account_info().clone(),
            authority: self.mint_authority.to_account_info().clone(),
        };
        CpiContext::new(self.token_program.to_account_info().clone(), cpi_accounts)
    }
}

#[account]
pub struct GlobalState {
    pub authority: Pubkey,
    pub hfrt_mint: Pubkey,
    pub fee_discount: u8,
    pub bump: u8,
}
impl GlobalState {
    /// Space: Pubkey (32) + Pubkey (32) + u8 (1) + u8 (1)
    pub const LEN: usize = 32 + 32 + 1 + 1;
}

#[account]
pub struct Governance {
    pub authority: Pubkey,
    pub rebate_rate: u8,      // For example: 10 means a 1% rebate.
    pub max_fee_discount: u8, // Maximum fee discount allowed.
}
impl Governance {
    /// Space: Pubkey (32) + u8 (1) + u8 (1)
    pub const LEN: usize = 32 + 1 + 1;
}

#[account]
pub struct Trader {
    pub owner: Pubkey,
    pub rolling_volume: u64,
    pub last_update: i64,
    pub staked_amount: u64,
    pub stake_start_time: i64, // Unix timestamp for when staking began.
}
impl Trader {
    /// Space: Pubkey (32) + u64 (8) + i64 (8) + u64 (8) + i64 (8)
    pub const LEN: usize = 32 + 8 + 8 + 8 + 8;
}

#[account]
pub struct DAOProposal {
    pub proposal_id: u64,
    pub proposer: Pubkey,
    pub new_fee_discount: u8,
    pub votes_for: u64,
    pub votes_against: u64,
}
impl DAOProposal {
    pub const LEN: usize = 8 + 32 + 1 + 8 + 8;
}

#[error_code]
pub enum ErrorCode {
    #[msg("Arithmetic overflow occurred.")]
    Overflow,
    #[msg("Insufficient staked tokens for unstaking.")]
    InsufficientStake,
    #[msg("Invalid rebate rate.")]
    InvalidRebateRate,
    #[msg("Potential wash trading detected.")]
    WashTrade,
    #[msg("Trades are occurring too frequently.")]
    FrequentTrades,
    #[msg("DAO proposal rejected due to insufficient votes.")]
    ProposalRejected,
}

#[event]
pub struct TradeRecorded {
    pub owner: Pubkey,
    pub trade_amount: u64,
    pub rolling_volume: u64,
}

#[event]
pub struct RebateClaimed {
    pub owner: Pubkey,
    pub rebate_amount: u64,
}
