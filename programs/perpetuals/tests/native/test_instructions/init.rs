use {
    crate::utils::{self, pda},
    anchor_lang::{
        prelude::{AccountMeta, Clock, Pubkey},
        ToAccountMetas,
    },
    perpetuals::{
        adapters::spl_governance_program_adapter,
        instructions::InitParams,
        state::{
            cortex::{Cortex, StakingRound},
            multisig::Multisig,
            perpetuals::Perpetuals,
        },
    },
    solana_program_test::{BanksClientError, ProgramTestContext},
    solana_sdk::signer::{keypair::Keypair, Signer},
};

pub async fn init(
    program_test_ctx: &mut ProgramTestContext,
    upgrade_authority: &Keypair,
    params: InitParams,
    governance_realm_pda: &Pubkey,
    stake_reward_token_mint: &Pubkey,
    multisig_signers: &[&Keypair],
) -> std::result::Result<(), BanksClientError> {
    // ==== WHEN ==============================================================
    let perpetuals_program_data_pda = pda::get_program_data_pda().0;
    let (multisig_pda, multisig_bump) = pda::get_multisig_pda();
    let (transfer_authority_pda, transfer_authority_bump) = pda::get_transfer_authority_pda();
    let (perpetuals_pda, perpetuals_bump) = pda::get_perpetuals_pda();
    let (cortex_pda, cortex_bump) = pda::get_cortex_pda();
    let (lm_token_mint_pda, lm_token_mint_bump) = pda::get_lm_token_mint_pda();
    let (governance_token_mint_pda, governance_token_mint_bump) =
        pda::get_governance_token_mint_pda();
    let (stake_token_account_pda, stake_token_account_bump) = pda::get_stake_token_account_pda();
    let (stake_reward_token_account_pda, stake_reward_token_account_bump) =
        pda::get_stake_reward_token_account_pda();
    let (stake_lm_reward_token_account_pda, stake_lm_reward_token_account_bump) =
        pda::get_stake_lm_reward_token_account_pda();

    let accounts_meta = {
        let accounts = perpetuals::accounts::Init {
            upgrade_authority: upgrade_authority.pubkey(),
            multisig: multisig_pda,
            transfer_authority: transfer_authority_pda,
            cortex: cortex_pda,
            lm_token_mint: lm_token_mint_pda,
            governance_token_mint: governance_token_mint_pda,
            stake_token_account: stake_token_account_pda,
            stake_reward_token_account: stake_reward_token_account_pda,
            stake_lm_reward_token_account: stake_lm_reward_token_account_pda,
            perpetuals: perpetuals_pda,
            perpetuals_program: perpetuals::ID,
            perpetuals_program_data: perpetuals_program_data_pda,
            governance_realm: *governance_realm_pda,
            governance_program: spl_governance_program_adapter::ID,
            stake_reward_token_mint: *stake_reward_token_mint,
            system_program: anchor_lang::system_program::ID,
            token_program: anchor_spl::token::ID,
        };

        let mut accounts_meta = accounts.to_account_metas(None);

        for signer in multisig_signers {
            accounts_meta.push(AccountMeta {
                pubkey: signer.pubkey(),
                is_signer: true,
                is_writable: false,
            });
        }

        accounts_meta
    };

    utils::create_and_execute_perpetuals_ix(
        program_test_ctx,
        accounts_meta,
        perpetuals::instruction::Init { params },
        Some(&upgrade_authority.pubkey()),
        &[&[upgrade_authority], multisig_signers].concat(),
    )
    .await?;

    // ==== THEN ==============================================================
    let perpetuals_account =
        utils::get_account::<Perpetuals>(program_test_ctx, perpetuals_pda).await;

    // Assert permissions
    {
        let p = perpetuals_account.permissions;

        assert_eq!(p.allow_swap, params.allow_swap);
        assert_eq!(p.allow_add_liquidity, params.allow_add_liquidity);
        assert_eq!(p.allow_remove_liquidity, params.allow_remove_liquidity);
        assert_eq!(p.allow_open_position, params.allow_open_position);
        assert_eq!(p.allow_close_position, params.allow_close_position);
        assert_eq!(p.allow_pnl_withdrawal, params.allow_pnl_withdrawal);
        assert_eq!(
            p.allow_collateral_withdrawal,
            params.allow_collateral_withdrawal
        );
        assert_eq!(p.allow_size_change, params.allow_size_change);
    }

    assert_eq!(
        perpetuals_account.transfer_authority_bump,
        transfer_authority_bump
    );
    assert_eq!(perpetuals_account.perpetuals_bump, perpetuals_bump);

    let cortex_account = utils::get_account::<Cortex>(program_test_ctx, cortex_pda).await;
    // Assert cortex
    {
        let clock = program_test_ctx.banks_client.get_sysvar::<Clock>().await?;
        assert_eq!(cortex_account.bump, cortex_bump);
        assert_eq!(cortex_account.lm_token_bump, lm_token_mint_bump);
        assert_eq!(
            cortex_account.governance_token_bump,
            governance_token_mint_bump
        );
        assert_eq!(cortex_account.inception_epoch, 0);
        assert_eq!(
            cortex_account.stake_token_account_bump,
            stake_token_account_bump
        );
        assert_eq!(
            cortex_account.stake_reward_token_account_bump,
            stake_reward_token_account_bump
        );
        assert_eq!(
            cortex_account.stake_lm_reward_token_account_bump,
            stake_lm_reward_token_account_bump
        );
        assert_eq!(
            cortex_account.stake_reward_token_mint,
            *stake_reward_token_mint
        );
        assert_eq!(
            cortex_account.current_staking_round,
            StakingRound::new(clock.unix_timestamp)
        );
        assert_eq!(cortex_account.next_staking_round, StakingRound::new(0));
        assert_eq!(cortex_account.resolved_staking_rounds.len(), 0);
    }

    let multisig_account = utils::get_account::<Multisig>(program_test_ctx, multisig_pda).await;
    // Assert multisig
    {
        assert_eq!(multisig_account.bump, multisig_bump);
        assert_eq!(multisig_account.min_signatures, params.min_signatures);

        // Check signers
        {
            for (i, signer) in multisig_signers.iter().enumerate() {
                assert_eq!(multisig_account.signers[i], signer.pubkey());
            }
        }
    }

    Ok(())
}
