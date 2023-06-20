//! RemoveLockedStake instruction handler

use {
    crate::{
        adapters::SplGovernanceV3Adapter,
        error::PerpetualsError,
        program,
        state::{
            cortex::Cortex,
            perpetuals::Perpetuals,
            staking::{Staking, STAKING_THREAD_AUTHORITY_SEED},
        },
    },
    anchor_lang::prelude::*,
    anchor_spl::token::{Mint, Token, TokenAccount},
};

#[derive(Accounts)]
pub struct RemoveLockedStake<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,

    #[account(
        mut,
        token::mint = lm_token_mint,
        has_one = owner
    )]
    pub lm_token_account: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        token::mint = staking_reward_token_mint,
        has_one = owner
    )]
    pub reward_token_account: Box<Account<'info, TokenAccount>>,

    // staked token vault
    #[account(
        mut,
        token::mint = lm_token_mint,
        token::authority = transfer_authority,
        seeds = [b"staking_token_account"],
        bump = cortex.staking_token_account_bump
    )]
    pub staking_token_account: Box<Account<'info, TokenAccount>>,

    // staking reward token vault
    #[account(
        mut,
        token::mint = staking_reward_token_mint,
        seeds = [b"staking_reward_token_account"],
        bump = cortex.staking_reward_token_account_bump
    )]
    pub staking_reward_token_account: Box<Account<'info, TokenAccount>>,

    // staking lm reward token vault
    #[account(
        mut,
        token::mint = lm_token_mint,
        seeds = [b"staking_lm_reward_token_account"],
        bump = cortex.staking_lm_reward_token_account_bump
    )]
    pub staking_lm_reward_token_account: Box<Account<'info, TokenAccount>>,

    /// CHECK: empty PDA, authority for token accounts
    #[account(
        seeds = [b"transfer_authority"],
        bump = perpetuals.transfer_authority_bump
    )]
    pub transfer_authority: AccountInfo<'info>,

    #[account(
        mut,
        seeds = [b"staking",
                 owner.key().as_ref()],
        bump = staking.bump
    )]
    pub staking: Box<Account<'info, Staking>>,

    #[account(
        mut,
        seeds = [b"cortex"],
        bump = cortex.bump,
        has_one = staking_reward_token_mint
    )]
    pub cortex: Box<Account<'info, Cortex>>,

    #[account(
        seeds = [b"perpetuals"],
        bump = perpetuals.perpetuals_bump
    )]
    pub perpetuals: Box<Account<'info, Perpetuals>>,

    #[account(
        mut,
        seeds = [b"lm_token_mint"],
        bump = cortex.lm_token_bump
    )]
    pub lm_token_mint: Box<Account<'info, Mint>>,

    #[account(
        mut,
        seeds = [b"governance_token_mint"],
        bump = cortex.governance_token_bump
    )]
    pub governance_token_mint: Box<Account<'info, Mint>>,

    #[account()]
    pub staking_reward_token_mint: Box<Account<'info, Mint>>,

    /// CHECK: checked by spl governance v3 program
    /// A realm represent one project (ADRENA, MANGO etc.) within the governance program
    pub governance_realm: UncheckedAccount<'info>,

    /// CHECK: checked by spl governance v3 program
    pub governance_realm_config: UncheckedAccount<'info>,

    /// CHECK: checked by spl governance v3 program
    /// Token account owned by governance program holding user's locked tokens
    #[account(mut)]
    pub governance_governing_token_holding: UncheckedAccount<'info>,

    /// CHECK: checked by spl governance v3 program
    /// Account owned by governance storing user informations
    #[account(mut)]
    pub governance_governing_token_owner_record: UncheckedAccount<'info>,

    /// CHECK: checked by clockwork thread program
    #[account(mut)]
    pub stakes_claim_cron_thread: Box<Account<'info, clockwork_sdk::state::Thread>>,

    /// CHECK: empty PDA, authority for threads
    #[account(
        seeds = [STAKING_THREAD_AUTHORITY_SEED, owner.key().as_ref()],
        bump = staking.thread_authority_bump
    )]
    pub staking_thread_authority: AccountInfo<'info>,

    clockwork_program: Program<'info, clockwork_sdk::ThreadProgram>,
    governance_program: Program<'info, SplGovernanceV3Adapter>,
    perpetuals_program: Program<'info, program::Perpetuals>,
    system_program: Program<'info, System>,
    token_program: Program<'info, Token>,
}

#[derive(AnchorSerialize, AnchorDeserialize, Copy, Clone)]
pub struct RemoveLockedStakeParams {
    pub locked_stake_index: usize,
}

// Remove one stake at a time
pub fn remove_locked_stake(
    ctx: Context<RemoveLockedStake>,
    params: &RemoveLockedStakeParams,
) -> Result<()> {
    // claim existing rewards before removing the stake
    {
        let cpi_accounts = crate::cpi::accounts::ClaimStakes {
            caller: ctx.accounts.owner.to_account_info(),
            payer: ctx.accounts.owner.to_account_info(),
            owner: ctx.accounts.owner.to_account_info(),
            reward_token_account: ctx.accounts.reward_token_account.to_account_info(),
            lm_token_account: ctx.accounts.lm_token_account.to_account_info(),
            staking_reward_token_account: ctx
                .accounts
                .staking_reward_token_account
                .to_account_info(),
            staking_lm_reward_token_account: ctx
                .accounts
                .staking_lm_reward_token_account
                .to_account_info(),
            transfer_authority: ctx.accounts.transfer_authority.to_account_info(),
            staking: ctx.accounts.staking.to_account_info(),
            cortex: ctx.accounts.cortex.to_account_info(),
            perpetuals: ctx.accounts.perpetuals.to_account_info(),
            lm_token_mint: ctx.accounts.lm_token_mint.to_account_info(),
            staking_reward_token_mint: ctx.accounts.staking_reward_token_mint.to_account_info(),
            perpetuals_program: ctx.accounts.perpetuals_program.to_account_info(),
            system_program: ctx.accounts.system_program.to_account_info(),
            token_program: ctx.accounts.token_program.to_account_info(),
        };

        let cpi_program = ctx.accounts.perpetuals_program.to_account_info();
        crate::cpi::claim_stakes(CpiContext::new(cpi_program, cpi_accounts))?
    }

    let staking = ctx.accounts.staking.as_mut();

    let token_amount_to_unstake = {
        let locked_stake = staking
            .locked_stakes
            .get(params.locked_stake_index)
            .ok_or(PerpetualsError::CannotFoundStake)?;

        // Check the stake have ended and have been resolved
        {
            let current_time = ctx.accounts.perpetuals.get_time()?;

            require!(
                locked_stake.has_ended(current_time) && locked_stake.resolved,
                PerpetualsError::UnresolvedStake
            );
        }

        let token_amount_to_unstake = locked_stake.amount;

        // Remove the stake from the list
        staking.locked_stakes.remove(params.locked_stake_index);

        token_amount_to_unstake
    };

    // Unstake owner's tokens
    {
        msg!("Transfer tokens");
        let perpetuals = ctx.accounts.perpetuals.as_mut();

        perpetuals.transfer_tokens(
            ctx.accounts.staking_token_account.to_account_info(),
            ctx.accounts.lm_token_account.to_account_info(),
            ctx.accounts.transfer_authority.to_account_info(),
            ctx.accounts.token_program.to_account_info(),
            token_amount_to_unstake,
        )?;
    }

    // pause auto-claim if there are no more staked token,
    {
        if !ctx.accounts.stakes_claim_cron_thread.paused
            && staking.liquid_stake.amount == 0
            && staking.locked_stakes.is_empty()
        {
            clockwork_sdk::cpi::thread_pause(CpiContext::new_with_signer(
                ctx.accounts.clockwork_program.to_account_info(),
                clockwork_sdk::cpi::ThreadPause {
                    authority: ctx.accounts.staking_thread_authority.to_account_info(),
                    thread: ctx.accounts.stakes_claim_cron_thread.to_account_info(),
                },
                &[&[
                    STAKING_THREAD_AUTHORITY_SEED,
                    ctx.accounts.owner.key().as_ref(),
                    &[ctx.accounts.staking.thread_authority_bump],
                ]],
            ))?;
        }
    }

    Ok(())
}
