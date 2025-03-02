# hfrt
# High-Frequency Trading Rebate Token (HFRT)

The **High-Frequency Trading Rebate Token (HFRT)** is a **Solana-based on-chain rebate system** designed to incentivize traders, liquidity providers, and market makers. It offers **fee discounts**, **execution priority**, and **staking rewards** for high-volume traders and active liquidity providers.

**devnet**:(https://explorer.solana.com/address/A86NRtxqJiyKm4da9jmA1TH1erjUG3ULcPXhS6wdyQk7?cluster=devnet)

## 🚀 Features

- **Rebate System**: Traders earn HFRT tokens based on their **24-hour rolling trade volume**.
- **Fee Discounts**: HFRT **stakers** receive **discounted trading fees** on swaps and limit orders.
- **Execution Priority**: Traders with higher HFRT holdings get **priority execution**.
- **Liquidity Provider Rewards**: Liquidity providers (LPs) receive **HFRT rewards** for maintaining **low-slippage pools**.
- **Auto-Compounding Rewards**: Traders can automatically reinvest their rewards into **staking**.
- **Sybil Resistance**: Protects against wash trading and flash-loan-based **fake volume**.
- **Governance & DAO**: HFRT holders can **vote on fee discounts** and protocol changes.

## 📜 Smart Contract(Program) Overview

### **Key Accounts**
| **Account**       | **Description** |
|------------------|---------------|
| `GlobalState` | Stores global settings like fee discounts and mint authority. |
| `Governance` | Manages rebate rates and discount governance. |
| `Trader` | Tracks each trader's **rolling volume**, **staked amount**, and **last trade time**. |
| `StakingVault` | Holds HFRT tokens staked by users. |
| `DAOProposal` | Allows HFRT holders to propose and vote on **fee discount changes**. |

### **Main Instructions**
| **Function** | **Description** |
|-------------|---------------|
| `initialize()` | Initializes the global state and HFRT mint. |
| `initialize_governance()` | Creates the governance account. |
| `record_trade(amount)` | Records a trade and updates the **rolling volume**. |
| `claim_rebate()` | Mints HFRT tokens based on a trader’s volume. |
| `stake_tokens(amount)` | Stakes HFRT tokens for **fee discounts**. |
| `unstake_tokens(amount)` | Withdraws staked HFRT with **dynamic penalties**. |
| `auto_compound()` | Mints HFRT rewards directly to the staking vault. |
| `create_dao_proposal(new_fee_discount)` | Proposes a fee discount change. |
| `vote_dao_proposal(vote_for: boolean)` | Votes on a proposal. |
| `execute_dao_proposal()` | Executes a passed proposal, updating the **fee discount**. |
