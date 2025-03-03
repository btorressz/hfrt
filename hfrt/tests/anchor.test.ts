// No imports needed: web3, anchor, pg and more are globally available

describe("HFRT Tests", () => {
  it("initialize", async () => {
    // Derive the global state PDA using the seed "global-state"
    const [globalStatePda] = await web3.PublicKey.findProgramAddress(
      [Buffer.from("global-state")],
      pg.program.programId
    );

    // SPL Token Program ID (manually set since TOKEN_PROGRAM_ID isn't available)
    const TOKEN_PROGRAM_ID = new web3.PublicKey("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");

    // Generate a new keypair for HFRT mint
    const hfrtMint = new web3.Keypair();

    // Create the HFRT mint account
    const txHashMint = await pg.program.provider.sendAndConfirm(
      new web3.Transaction().add(
        web3.SystemProgram.createAccount({
          fromPubkey: pg.wallet.publicKey,
          newAccountPubkey: hfrtMint.publicKey,
          space: 82, // Standard space for SPL token mints
          lamports: await pg.connection.getMinimumBalanceForRentExemption(82),
          programId: TOKEN_PROGRAM_ID,
        })
      ),
      [hfrtMint]
    );

    console.log(`HFRT Mint Created - TX: ${txHashMint}`);

    // Define the fee discount value (example: 10)
    const feeDiscount = 10;

    // Initialize HFRT global state
    const txHash = await pg.program.methods
      .initialize(feeDiscount)
      .accounts({
        globalState: globalStatePda,
        authority: pg.wallet.publicKey,
        hfrtMint: hfrtMint.publicKey,
        systemProgram: web3.SystemProgram.programId,
        rent: web3.SYSVAR_RENT_PUBKEY,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .rpc();

    console.log(`HFRT Initialized - TX: ${txHash}`);

    // Fetch the on-chain global state account
    const globalState = await pg.program.account.globalState.fetch(globalStatePda);

    console.log("On-chain Global State:", globalState);

    // Verify global state matches expected values
    assert.equal(globalState.feeDiscount, feeDiscount);
    assert(globalState.authority.equals(pg.wallet.publicKey));
    assert(globalState.hfrtMint.equals(hfrtMint.publicKey));
  });
});

