//! OpenPosition instruction handler

use {
    crate::{
        error::PerpetualsError,
        instructions::{BucketName, MintLmTokensFromBucketParams, SwapParams},
        math,
        state::{
            cortex::Cortex,
            custody::Custody,
            oracle::OraclePrice,
            perpetuals::Perpetuals,
            pool::Pool,
            position::{Position, Side},
            staking::Staking,
        },
    },
    anchor_lang::prelude::*,
    anchor_spl::token::{Mint, Token, TokenAccount},
    num_traits::Zero,
    solana_program::program_error::ProgramError,
};

#[derive(Accounts)]
#[instruction(params: OpenPositionParams)]
pub struct OpenPosition<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,

    #[account(
        mut,
        constraint = funding_account.mint == collateral_custody.mint,
        has_one = owner
    )]
    pub funding_account: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        constraint = lm_token_account.mint == lm_token_mint.key(),
        has_one = owner
    )]
    pub lm_token_account: Box<Account<'info, TokenAccount>>,

    /// CHECK: empty PDA, authority for token accounts
    #[account(
        seeds = [b"transfer_authority"],
        bump = perpetuals.transfer_authority_bump
    )]
    pub transfer_authority: AccountInfo<'info>,

    #[account(
        mut,
        seeds = [b"staking", lm_staking.staked_token_mint.as_ref()],
        bump = lm_staking.bump,
        constraint = lm_staking.reward_token_mint.key() == staking_reward_token_mint.key()
    )]
    pub lm_staking: Box<Account<'info, Staking>>,

    #[account(
        mut,
        seeds = [b"staking", lp_staking.staked_token_mint.as_ref()],
        bump = lp_staking.bump,
        constraint = lp_staking.reward_token_mint.key() == staking_reward_token_mint.key()
    )]
    pub lp_staking: Box<Account<'info, Staking>>,

    #[account(
        mut,
        seeds = [b"cortex"],
        bump = cortex.bump,
    )]
    pub cortex: Box<Account<'info, Cortex>>,

    #[account(
        seeds = [b"perpetuals"],
        bump = perpetuals.perpetuals_bump
    )]
    pub perpetuals: Box<Account<'info, Perpetuals>>,

    #[account(
        mut,
        seeds = [b"pool",
                 pool.name.as_bytes()],
        bump = pool.bump
    )]
    pub pool: Box<Account<'info, Pool>>,

    #[account(
        init,
        payer = owner,
        space = Position::LEN,
        seeds = [b"position",
                 owner.key().as_ref(),
                 pool.key().as_ref(),
                 custody.key().as_ref(),
                 &[params.side as u8]],
        bump
    )]
    pub position: Box<Account<'info, Position>>,

    #[account(
        mut,
        seeds = [b"custody",
                 pool.key().as_ref(),
                 staking_reward_token_custody.mint.as_ref()],
        bump = staking_reward_token_custody.bump,
        constraint = staking_reward_token_custody.mint == staking_reward_token_mint.key(),
    )]
    pub staking_reward_token_custody: Box<Account<'info, Custody>>,

    /// CHECK: oracle account for the stake_reward token
    #[account(
        constraint = staking_reward_token_custody_oracle_account.key() == staking_reward_token_custody.oracle.oracle_account
    )]
    pub staking_reward_token_custody_oracle_account: AccountInfo<'info>,

    #[account(
        mut,
        seeds = [b"custody_token_account",
                 pool.key().as_ref(),
                 staking_reward_token_custody.mint.as_ref()],
        bump = staking_reward_token_custody.token_account_bump,
    )]
    pub staking_reward_token_custody_token_account: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        seeds = [b"custody",
                 pool.key().as_ref(),
                 custody.mint.as_ref()],
        bump = custody.bump
    )]
    pub custody: Box<Account<'info, Custody>>,

    /// CHECK: oracle account for the position token
    #[account(
        constraint = custody_oracle_account.key() == custody.oracle.oracle_account
    )]
    pub custody_oracle_account: AccountInfo<'info>,

    #[account(
        mut,
        seeds = [b"custody",
                 pool.key().as_ref(),
                 collateral_custody.mint.as_ref()],
        bump = collateral_custody.bump
    )]
    pub collateral_custody: Box<Account<'info, Custody>>,

    /// CHECK: oracle account for the collateral token
    #[account(
        constraint = collateral_custody_oracle_account.key() == collateral_custody.oracle.oracle_account
    )]
    pub collateral_custody_oracle_account: AccountInfo<'info>,

    #[account(
        mut,
        seeds = [b"custody_token_account",
                 pool.key().as_ref(),
                 collateral_custody.mint.as_ref()],
        bump = collateral_custody.token_account_bump
    )]
    pub collateral_custody_token_account: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        token::mint = lm_staking.reward_token_mint,
        seeds = [b"staking_reward_token_vault", lm_staking.key().as_ref()],
        bump = lm_staking.reward_token_vault_bump
    )]
    pub lm_staking_reward_token_vault: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        token::mint = lp_staking.reward_token_mint,
        seeds = [b"staking_reward_token_vault", lp_staking.key().as_ref()],
        bump = lp_staking.reward_token_vault_bump
    )]
    pub lp_staking_reward_token_vault: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        seeds = [b"lm_token_mint"],
        bump = cortex.lm_token_bump
    )]
    pub lm_token_mint: Box<Account<'info, Mint>>,

    #[account(
        mut,
        seeds = [b"lp_token_mint",
                 pool.key().as_ref()],
        bump = pool.lp_token_bump
    )]
    pub lp_token_mint: Box<Account<'info, Mint>>,

    #[account()]
    pub staking_reward_token_mint: Box<Account<'info, Mint>>,

    system_program: Program<'info, System>,
    token_program: Program<'info, Token>,
    perpetuals_program: Program<'info, Perpetuals>,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy)]
pub struct OpenPositionParams {
    pub price: u64,
    pub collateral: u64,
    pub size: u64,
    pub side: Side,
}

pub fn open_position(ctx: Context<OpenPosition>, params: &OpenPositionParams) -> Result<()> {
    // check permissions
    msg!("Check permissions");
    let perpetuals = ctx.accounts.perpetuals.as_mut();
    let custody = ctx.accounts.custody.as_mut();
    let collateral_custody = ctx.accounts.collateral_custody.as_mut();
    require!(
        perpetuals.permissions.allow_open_position
            && custody.permissions.allow_open_position
            && !custody.is_stable,
        PerpetualsError::InstructionNotAllowed
    );

    // validate inputs
    msg!("Validate inputs");
    if params.price == 0 || params.collateral == 0 || params.size == 0 || params.side == Side::None
    {
        return Err(ProgramError::InvalidArgument.into());
    }
    if params.side == Side::Short || custody.is_virtual {
        require_keys_neq!(custody.key(), collateral_custody.key());
        require!(
            collateral_custody.is_stable && !collateral_custody.is_virtual,
            PerpetualsError::InvalidCollateralCustody
        );
    } else {
        require_keys_eq!(custody.key(), collateral_custody.key());
    };
    let position = ctx.accounts.position.as_mut();
    let pool = ctx.accounts.pool.as_mut();

    // compute position price
    let curtime = perpetuals.get_time()?;

    let token_price = OraclePrice::new_from_oracle(
        &ctx.accounts.custody_oracle_account.to_account_info(),
        &custody.oracle,
        curtime,
        false,
    )?;

    let token_ema_price = OraclePrice::new_from_oracle(
        &ctx.accounts.custody_oracle_account.to_account_info(),
        &custody.oracle,
        curtime,
        custody.pricing.use_ema,
    )?;

    let max_price = if token_price > token_ema_price {
        token_price
    } else {
        token_ema_price
    };

    let collateral_token_price = OraclePrice::new_from_oracle(
        &ctx.accounts
            .collateral_custody_oracle_account
            .to_account_info(),
        &collateral_custody.oracle,
        curtime,
        false,
    )?;

    let collateral_token_ema_price = OraclePrice::new_from_oracle(
        &ctx.accounts
            .collateral_custody_oracle_account
            .to_account_info(),
        &collateral_custody.oracle,
        curtime,
        collateral_custody.pricing.use_ema,
    )?;

    let min_collateral_price = collateral_token_price
        .get_min_price(&collateral_token_ema_price, collateral_custody.is_stable)?;

    let position_price =
        pool.get_entry_price(&token_price, &token_ema_price, params.side, custody)?;
    msg!("Entry price: {}", position_price);

    if params.side == Side::Long {
        require_gte!(
            params.price,
            position_price,
            PerpetualsError::MaxPriceSlippage
        );
    } else {
        require_gte!(
            position_price,
            params.price,
            PerpetualsError::MaxPriceSlippage
        );
    }

    // compute position parameters
    let size_usd = max_price.get_asset_amount_usd(params.size, custody.decimals)?;
    let collateral_usd = min_collateral_price
        .get_asset_amount_usd(params.collateral, collateral_custody.decimals)?;

    let locked_amount = if params.side == Side::Short || custody.is_virtual {
        custody.get_locked_amount(
            min_collateral_price.get_token_amount(size_usd, collateral_custody.decimals)?,
        )?
    } else {
        custody.get_locked_amount(params.size)?
    };

    // compute fee
    let fee_amount = pool.get_entry_fee(
        custody.fees.open_position,
        params.size,
        locked_amount,
        collateral_custody,
    )?;
    msg!("Collected fee: {}", fee_amount);

    // compute amount to transfer
    let transfer_amount = math::checked_add(params.collateral, fee_amount)?;
    msg!("Amount in: {}", transfer_amount);

    // init new position
    msg!("Initialize new position");
    position.owner = ctx.accounts.owner.key();
    position.pool = pool.key();
    position.custody = custody.key();
    position.collateral_custody = collateral_custody.key();
    position.open_time = perpetuals.get_time()?;
    position.update_time = 0;
    position.side = params.side;
    position.price = position_price;
    position.size_usd = size_usd;
    position.collateral_usd = collateral_usd;
    position.unrealized_profit_usd = 0;
    position.unrealized_loss_usd = 0;
    position.cumulative_interest_snapshot = collateral_custody.get_cumulative_interest(curtime)?;
    position.locked_amount = locked_amount;
    position.collateral_amount = params.collateral;
    position.bump = *ctx
        .bumps
        .get("position")
        .ok_or(ProgramError::InvalidSeeds)?;

    // check position risk
    msg!("Check position risks");
    require!(
        position.locked_amount > 0,
        PerpetualsError::InsufficientAmountReturned
    );

    require!(
        pool.check_leverage(
            position,
            &token_price,
            &token_ema_price,
            custody,
            &collateral_token_price,
            &collateral_token_ema_price,
            collateral_custody,
            curtime,
            true
        )?,
        PerpetualsError::MaxLeverage
    );

    // lock funds for potential profit payoff
    collateral_custody.lock_funds(position.locked_amount)?;

    // transfer tokens
    msg!("Transfer tokens");
    perpetuals.transfer_tokens_from_user(
        ctx.accounts.funding_account.to_account_info(),
        ctx.accounts
            .collateral_custody_token_account
            .to_account_info(),
        ctx.accounts.owner.to_account_info(),
        ctx.accounts.token_program.to_account_info(),
        transfer_amount,
    )?;

    // LM rewards
    let lm_rewards_amount = {
        // compute amount of lm token to mint
        let amount = ctx.accounts.cortex.get_lm_rewards_amount(fee_amount)?;

        if amount > 0 {
            let cpi_accounts = crate::cpi::accounts::MintLmTokensFromBucket {
                admin: ctx.accounts.transfer_authority.to_account_info(),
                receiving_account: ctx.accounts.lm_token_account.to_account_info(),
                transfer_authority: ctx.accounts.transfer_authority.to_account_info(),
                cortex: ctx.accounts.cortex.to_account_info(),
                perpetuals: perpetuals.to_account_info(),
                lm_token_mint: ctx.accounts.lm_token_mint.to_account_info(),
                token_program: ctx.accounts.token_program.to_account_info(),
            };

            let cpi_program = ctx.accounts.perpetuals_program.to_account_info();
            crate::cpi::mint_lm_tokens_from_bucket(
                CpiContext::new_with_signer(
                    cpi_program,
                    cpi_accounts,
                    &[&[b"transfer_authority", &[perpetuals.transfer_authority_bump]]],
                ),
                MintLmTokensFromBucketParams {
                    bucket_name: BucketName::Ecosystem,
                    amount,
                    reason: String::from("Liquidity mining rewards"),
                },
            )?;

            {
                ctx.accounts.lm_token_account.reload()?;
                ctx.accounts.cortex.reload()?;
                perpetuals.reload()?;
                ctx.accounts.lm_token_mint.reload()?;
            }
        }

        msg!("Amount LM rewards out: {}", amount);
        amount
    };

    // update custody stats
    msg!("Update custody stats");
    collateral_custody.collected_fees.open_position_usd = collateral_custody
        .collected_fees
        .open_position_usd
        .wrapping_add(
            collateral_token_ema_price
                .get_asset_amount_usd(fee_amount, collateral_custody.decimals)?,
        );

    custody.distributed_rewards.open_position_lm = custody
        .distributed_rewards
        .open_position_lm
        .wrapping_add(lm_rewards_amount);

    collateral_custody.assets.collateral =
        math::checked_add(collateral_custody.assets.collateral, params.collateral)?;

    let protocol_fee = Pool::get_fee_amount(custody.fees.protocol_share, fee_amount)?;
    collateral_custody.assets.protocol_fees =
        math::checked_add(collateral_custody.assets.protocol_fees, protocol_fee)?;

    // if custody and collateral_custody accounts are the same, ensure that data is in sync
    if position.side == Side::Long && !custody.is_virtual {
        collateral_custody.volume_stats.open_position_usd = collateral_custody
            .volume_stats
            .open_position_usd
            .wrapping_add(size_usd);

        if params.side == Side::Long {
            collateral_custody.trade_stats.oi_long_usd =
                math::checked_add(collateral_custody.trade_stats.oi_long_usd, size_usd)?;
        } else {
            collateral_custody.trade_stats.oi_short_usd =
                math::checked_add(collateral_custody.trade_stats.oi_short_usd, size_usd)?;
        }

        collateral_custody.add_position(position, &token_ema_price, curtime, None)?;
        collateral_custody.update_borrow_rate(curtime)?;
        *custody = collateral_custody.clone();
    } else {
        custody.volume_stats.open_position_usd = custody
            .volume_stats
            .open_position_usd
            .wrapping_add(size_usd);

        if params.side == Side::Long {
            custody.trade_stats.oi_long_usd =
                math::checked_add(custody.trade_stats.oi_long_usd, size_usd)?;
        } else {
            custody.trade_stats.oi_short_usd =
                math::checked_add(custody.trade_stats.oi_short_usd, size_usd)?;
        }

        custody.add_position(
            position,
            &token_ema_price,
            curtime,
            Some(collateral_custody),
        )?;
        collateral_custody.update_borrow_rate(curtime)?;
    }

    //
    // Calculate fee distribution between (Staked LM, Locked Staked LP, Organic LP)
    //
    let fee_distribution = ctx.accounts.cortex.calculate_fee_distribution(
        fee_amount,
        ctx.accounts.lp_token_mint.as_ref(),
        ctx.accounts.lp_staking.as_ref(),
    )?;

    //
    // Redistribute fees
    //

    // redistribute to ADX stakers
    {
        if !fee_distribution.lm_stakers_fee.is_zero() {
            // It is possible that the custody targeted by the function and the stake_reward one are the same, in that
            // case we need to only use one else there are some complication when saving state at the end.
            //
            // if the collected fees are in the right denomination, skip swap
            if custody.mint == ctx.accounts.staking_reward_token_custody.mint {
                msg!("Transfer collected fees to stake vault (no swap)");
                perpetuals.transfer_tokens(
                    ctx.accounts
                        .collateral_custody_token_account
                        .to_account_info(),
                    ctx.accounts.lm_staking_reward_token_vault.to_account_info(),
                    ctx.accounts.transfer_authority.to_account_info(),
                    ctx.accounts.token_program.to_account_info(),
                    fee_distribution.lm_stakers_fee,
                )?;
            } else {
                // swap the collected fee_amount to stable and send to staking rewards
                msg!("Swap collected fees to stake reward mint internally");
                perpetuals.internal_swap(
                    ctx.accounts.transfer_authority.to_account_info(),
                    ctx.accounts
                        .collateral_custody_token_account
                        .to_account_info(),
                    ctx.accounts.lm_staking_reward_token_vault.to_account_info(),
                    ctx.accounts.lm_token_account.to_account_info(),
                    ctx.accounts.cortex.to_account_info(),
                    perpetuals.to_account_info(),
                    pool.to_account_info(),
                    custody.to_account_info(),
                    ctx.accounts.custody_oracle_account.to_account_info(),
                    ctx.accounts
                        .collateral_custody_token_account
                        .to_account_info(),
                    ctx.accounts.staking_reward_token_custody.to_account_info(),
                    ctx.accounts
                        .staking_reward_token_custody_oracle_account
                        .to_account_info(),
                    ctx.accounts
                        .staking_reward_token_custody_token_account
                        .to_account_info(),
                    ctx.accounts.staking_reward_token_custody.to_account_info(),
                    ctx.accounts
                        .staking_reward_token_custody_oracle_account
                        .to_account_info(),
                    ctx.accounts
                        .staking_reward_token_custody_token_account
                        .to_account_info(),
                    ctx.accounts.lm_staking_reward_token_vault.to_account_info(),
                    ctx.accounts.lp_staking_reward_token_vault.to_account_info(),
                    ctx.accounts.staking_reward_token_mint.to_account_info(),
                    ctx.accounts.lm_staking.to_account_info(),
                    ctx.accounts.lp_staking.to_account_info(),
                    ctx.accounts.lm_token_mint.to_account_info(),
                    ctx.accounts.lp_token_mint.to_account_info(),
                    ctx.accounts.token_program.to_account_info(),
                    ctx.accounts.perpetuals_program.to_account_info(),
                    SwapParams {
                        amount_in: fee_distribution.lm_stakers_fee,
                        min_amount_out: 0,
                    },
                )?;
            }
        }
    }

    // redistribute to ALP locked stakers
    {
        if !fee_distribution.locked_lp_stakers_fee.is_zero() {
            // It is possible that the custody targeted by the function and the stake_reward one are the same, in that
            // case we need to only use one else there are some complication when saving state at the end.
            //
            // if the collected fees are in the right denomination, skip swap
            if custody.mint == ctx.accounts.staking_reward_token_custody.mint {
                msg!("Transfer collected fees to stake vault (no swap)");
                perpetuals.transfer_tokens(
                    ctx.accounts
                        .collateral_custody_token_account
                        .to_account_info(),
                    ctx.accounts.lp_staking_reward_token_vault.to_account_info(),
                    ctx.accounts.transfer_authority.to_account_info(),
                    ctx.accounts.token_program.to_account_info(),
                    fee_distribution.locked_lp_stakers_fee,
                )?;
            } else {
                // swap the collected fee_amount to stable and send to staking rewards
                msg!("Swap collected fees to stake reward mint internally");
                perpetuals.internal_swap(
                    ctx.accounts.transfer_authority.to_account_info(),
                    ctx.accounts
                        .collateral_custody_token_account
                        .to_account_info(),
                    ctx.accounts.lp_staking_reward_token_vault.to_account_info(),
                    ctx.accounts.lm_token_account.to_account_info(),
                    ctx.accounts.cortex.to_account_info(),
                    perpetuals.to_account_info(),
                    pool.to_account_info(),
                    custody.to_account_info(),
                    ctx.accounts.custody_oracle_account.to_account_info(),
                    ctx.accounts
                        .collateral_custody_token_account
                        .to_account_info(),
                    ctx.accounts.staking_reward_token_custody.to_account_info(),
                    ctx.accounts
                        .staking_reward_token_custody_oracle_account
                        .to_account_info(),
                    ctx.accounts
                        .staking_reward_token_custody_token_account
                        .to_account_info(),
                    ctx.accounts.staking_reward_token_custody.to_account_info(),
                    ctx.accounts
                        .staking_reward_token_custody_oracle_account
                        .to_account_info(),
                    ctx.accounts
                        .staking_reward_token_custody_token_account
                        .to_account_info(),
                    ctx.accounts.lm_staking_reward_token_vault.to_account_info(),
                    ctx.accounts.lp_staking_reward_token_vault.to_account_info(),
                    ctx.accounts.staking_reward_token_mint.to_account_info(),
                    ctx.accounts.lm_staking.to_account_info(),
                    ctx.accounts.lp_staking.to_account_info(),
                    ctx.accounts.lm_token_mint.to_account_info(),
                    ctx.accounts.lp_token_mint.to_account_info(),
                    ctx.accounts.token_program.to_account_info(),
                    ctx.accounts.perpetuals_program.to_account_info(),
                    SwapParams {
                        amount_in: fee_distribution.locked_lp_stakers_fee,
                        min_amount_out: 0,
                    },
                )?;
            }
        }
    }

    Ok(())
}
