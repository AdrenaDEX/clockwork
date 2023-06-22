use {
    crate::utils::{self, pda},
    anchor_lang::{prelude::Pubkey, ToAccountMetas},
    bonfida_test_utils::ProgramTestContextExt,
    perpetuals::{
        instructions::OpenPositionParams,
        state::{custody::Custody, position::Position},
    },
    solana_program_test::{BanksClientError, ProgramTestContext},
    solana_sdk::signer::{keypair::Keypair, Signer},
};

pub async fn open_position(
    program_test_ctx: &mut ProgramTestContext,
    owner: &Keypair,
    payer: &Keypair,
    pool_pda: &Pubkey,
    custody_token_mint: &Pubkey,
    staking_reward_token_mint: &Pubkey,
    params: OpenPositionParams,
) -> std::result::Result<(Pubkey, u8), BanksClientError> {
    // ==== WHEN ==============================================================

    // Prepare PDA and addresses
    let transfer_authority_pda = pda::get_transfer_authority_pda().0;
    let perpetuals_pda = pda::get_perpetuals_pda().0;
    let custody_pda = pda::get_custody_pda(pool_pda, custody_token_mint).0;
    let custody_token_account_pda =
        pda::get_custody_token_account_pda(pool_pda, custody_token_mint).0;
    let cortex_pda = pda::get_cortex_pda().0;
    let lm_token_mint_pda = pda::get_lm_token_mint_pda().0;
    let (position_pda, position_bump) =
        pda::get_position_pda(&owner.pubkey(), pool_pda, &custody_pda, params.side);
    let staking_pda = pda::get_staking_pda(perpetuals::state::staking::StakingType::LM).0;

    let funding_account_address =
        utils::find_associated_token_account(&owner.pubkey(), custody_token_mint).0;
    let lm_token_account_address =
        utils::find_associated_token_account(&owner.pubkey(), &lm_token_mint_pda).0;

    let custody_account = utils::get_account::<Custody>(program_test_ctx, custody_pda).await;
    let custody_oracle_account_address = custody_account.oracle.oracle_account;

    let staking_reward_token_vault_pda = pda::get_staking_reward_token_vault_pda(&staking_pda).0;

    let srt_custody_pda = pda::get_custody_pda(pool_pda, staking_reward_token_mint).0;
    let srt_custody_token_account_pda =
        pda::get_custody_token_account_pda(pool_pda, staking_reward_token_mint).0;
    let srt_custody_account =
        utils::get_account::<Custody>(program_test_ctx, srt_custody_pda).await;
    let srt_custody_oracle_account_address = srt_custody_account.oracle.oracle_account;

    // Save account state before tx execution
    let owner_funding_account_before = program_test_ctx
        .get_token_account(funding_account_address)
        .await
        .unwrap();
    let owner_lm_token_account_before = program_test_ctx
        .get_token_account(lm_token_account_address)
        .await
        .unwrap();
    let custody_token_account_before = program_test_ctx
        .get_token_account(custody_token_account_pda)
        .await
        .unwrap();

    utils::create_and_execute_perpetuals_ix(
        program_test_ctx,
        perpetuals::accounts::OpenPosition {
            owner: owner.pubkey(),
            funding_account: funding_account_address,
            lm_token_account: lm_token_account_address,
            transfer_authority: transfer_authority_pda,
            staking: staking_pda,
            cortex: cortex_pda,
            perpetuals: perpetuals_pda,
            pool: *pool_pda,
            position: position_pda,
            custody: custody_pda,
            custody_oracle_account: custody_oracle_account_address,
            stake_reward_token_custody: srt_custody_pda,
            stake_reward_token_custody_oracle_account: srt_custody_oracle_account_address,
            stake_reward_token_custody_token_account: srt_custody_token_account_pda,
            staking_reward_token_vault: staking_reward_token_vault_pda, // the stake reward vault
            lm_token_mint: lm_token_mint_pda,
            staking_reward_token_mint: *staking_reward_token_mint,
            collateral_custody: custody_pda,
            collateral_custody_oracle_account: custody_oracle_account_address,
            collateral_custody_token_account: custody_token_account_pda,
            system_program: anchor_lang::system_program::ID,
            token_program: anchor_spl::token::ID,
            perpetuals_program: perpetuals::ID,
        }
        .to_account_metas(None),
        perpetuals::instruction::OpenPosition { params },
        Some(&payer.pubkey()),
        &[owner, payer],
    )
    .await?;

    // ==== THEN ==============================================================
    // Check the balance change
    {
        let owner_funding_account_after = program_test_ctx
            .get_token_account(funding_account_address)
            .await
            .unwrap();
        let owner_lm_token_account_after = program_test_ctx
            .get_token_account(lm_token_account_address)
            .await
            .unwrap();
        let custody_token_account_after = program_test_ctx
            .get_token_account(custody_token_account_pda)
            .await
            .unwrap();

        assert!(owner_funding_account_after.amount < owner_funding_account_before.amount);
        assert!(owner_lm_token_account_after.amount > owner_lm_token_account_before.amount);
        assert!(custody_token_account_after.amount > custody_token_account_before.amount);
    }

    // Check the position
    {
        let position_account = utils::get_account::<Position>(program_test_ctx, position_pda).await;

        assert_eq!(position_account.owner, owner.pubkey());
        assert_eq!(position_account.pool, *pool_pda);
        assert_eq!(position_account.custody, custody_pda);
        assert_eq!(
            position_account.open_time,
            utils::get_current_unix_timestamp(program_test_ctx).await
        );
        assert_eq!(position_account.update_time, 0);
        assert_eq!(position_account.side, params.side);
        assert_eq!(position_account.unrealized_profit_usd, 0);
        assert_eq!(position_account.unrealized_loss_usd, 0);
        assert_eq!(position_account.collateral_amount, params.collateral);
        assert_eq!(position_account.bump, position_bump);
    }

    Ok((position_pda, position_bump))
}
