use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{transfer, Mint, Token, TokenAccount, Transfer},
};

use solana_program::clock::Clock;
use solana_program::pubkey;

declare_id!("DwcMT84wQTaNnQF3jXtHkKKHu5DwzVY9XVKVGWoVXKNy");

pub mod constants {
    pub const VAULT_SEED: &[u8] = b"vault";
    pub const STAKE_INFO_SEED: &[u8] = b"stake_info";
    pub const TOKEN_SEED: &[u8] = b"token";
    pub const SLOTS_PER_DAY: u64 = 216000;
    pub const SLOTS_PER_WEEK: u64 = SLOTS_PER_DAY * 7;
    pub const SLOTS_PER_MONTH: u64 = SLOTS_PER_DAY * 30;
    pub const SLOTS_PER_YEAR: u64 = SLOTS_PER_DAY * 365;
}

#[program]
pub mod staking_program {
    use super::*;
    pub fn initialize(
        ctx: Context<Initialize>,
        lock_time: u64,
        apy: u64,
        apy_denominator: u64,
        roi_type: u64,
    ) -> Result<()> {
        // Validate input parameters

        if lock_time <= 0 {
            return Err(ErrorCode::InvalidLockTime.into());
        }
        if apy <= 0 {
            return Err(ErrorCode::InvalidApy.into());
        }
        if apy_denominator <= 0 {
            return Err(ErrorCode::InvalidApyDenominator.into());
        }
        if roi_type > 2 {
            return Err(ErrorCode::InvalidRoiType.into());
        }

        let pool_info = &mut ctx.accounts.pool_info;

        // Ensure the admin is set correctly
        if pool_info.admin != Pubkey::default() && pool_info.admin != ctx.accounts.admin.key() {
            return Err(ErrorCode::UnauthorizedAdmin.into());
        }

        //Setup pool info states
        pool_info.admin = ctx.accounts.admin.key();
        pool_info.token_vault = ctx.accounts.token_vault_account.key();
        pool_info.lock_time = lock_time;
        pool_info.apy = apy;
        pool_info.apy_denominator = apy_denominator;
        pool_info.roi_type = roi_type; // 0-> Daily, 1-> Weekly, 2-> Monthly
        pool_info.token = ctx.accounts.mint.key();

        Ok(())
    }

    pub fn stake(
        ctx: Context<Stake>,
        stake_counter: u64,
        amount: u64,
        autostake: bool,
    ) -> Result<()> {
        let stake_info = &mut ctx.accounts.stake_info_account;

        //check if stake_seed is unique
        if stake_info.stake_seed == stake_counter {
            return Err(ErrorCode::IsStakeSeed.into());
        }

        //check if user has already staked
        if stake_info.is_staked {
            return Err(ErrorCode::IsStaked.into());
        }

        //Ensure that the amount is a positive integer
        if amount <= 0 {
            return Err(ErrorCode::NoTokens.into());
        }

        //Ensure non re-entrance
        if stake_info.in_process {
            return Err(ErrorCode::AlreadyInProcess.into());
        }

        let clock = Clock::get()?;

        //Setup states before transfer
        stake_info.deposit_timestamp = clock.unix_timestamp;
        msg!("Deposit Timestamp: {}", stake_info.deposit_timestamp);
        stake_info.stake_at_slot = clock.slot;
        stake_info.is_staked = true;
        stake_info.in_process = true;
        stake_info.stake_seed = stake_counter;
        stake_info.autostake = autostake;
        let pool_info = &ctx.accounts.pool_info;
        let lock_time = pool_info.lock_time;
        let roi_type = pool_info.roi_type;

        let stake_amount = (amount)
            .checked_mul(10u64.pow(ctx.accounts.mint.decimals as u32))
            .unwrap();

        //transfer the tokens from user account to user stake account
        transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.user_token_account.to_account_info(),
                    to: ctx.accounts.stake_account.to_account_info(),
                    authority: ctx.accounts.signer.to_account_info(),
                },
            ),
            stake_amount,
        )?;

        //update remaining states of stake info account
        stake_info.staked_amount = stake_amount;
        stake_info.end_time = stake_info.stake_at_slot + lock_time;
        stake_info.unclaimed_rewards = 0;
        stake_info.last_interaction_time = clock.slot;
        stake_info.total_claimed = 0;
        stake_info.pool_info = pool_info.key();
        stake_info.claim_cycles_passed = 0;

        if roi_type == 0 {
            stake_info.next_claim_time = stake_info.stake_at_slot + constants::SLOTS_PER_DAY; // 1 day from now
            stake_info.total_claim_cycles = lock_time / constants::SLOTS_PER_DAY;
        } else if roi_type == 1 {
            stake_info.next_claim_time = stake_info.stake_at_slot + constants::SLOTS_PER_WEEK; // 7 days from now
            stake_info.total_claim_cycles = lock_time / constants::SLOTS_PER_WEEK;
        } else if roi_type == 2 {
            stake_info.next_claim_time = stake_info.stake_at_slot + constants::SLOTS_PER_MONTH; //30 days from now
            stake_info.total_claim_cycles = lock_time / constants::SLOTS_PER_MONTH;
        } else {
            // Optional: handle other roi_type values
            return Err(ErrorCode::InvalidRoiType.into());
        }

        stake_info.in_process = false;

        Ok(())
    }

    pub fn destake(ctx: Context<DeStake>, stake_counter: u64) -> Result<()> {
        let stake_info = &mut ctx.accounts.stake_info_account;
        let pool_info = &mut ctx.accounts.pool_info;

        //Ensure that the stake record exists for a user
        if !stake_info.is_staked {
            return Err(ErrorCode::NotStaked.into());
        }

        let clock = Clock::get()?;

        //Ensure that the user is not unstaking before lock period is over
        if clock.slot < stake_info.end_time {
            return Err(ErrorCode::StillLocked.into());
        }

        //Ensure non re-entrance
        if stake_info.in_process {
            return Err(ErrorCode::AlreadyInProcess.into());
        }

        stake_info.in_process = true;

        let stake_amount = ctx.accounts.stake_account.amount;

        if stake_info.autostake {
            // Determine cycle type (daily/weekly/monthly/etc)
            let cycle_duration = match pool_info.roi_type {
                0 => constants::SLOTS_PER_DAY,   // Daily reward calculation
                1 => constants::SLOTS_PER_WEEK,  // Weekly reward calculation
                2 => constants::SLOTS_PER_MONTH, // Monthly reward calculation
                _ => return Err(ErrorCode::InvalidRoiType.into()),
            };

            let total_cycles = stake_info.total_claim_cycles;
            let current_stake = stake_info.staked_amount;
            // Safely convert total_cycles to i32
            let total_cycles_i32 = total_cycles
                .try_into()
                .map_err(|_| ErrorCode::InvalidCycleCount)?;

            // Calculate the effective APY for the cycle
            let apy_per_cycle = pool_info.apy as f64 / pool_info.apy_denominator as f64
                * (cycle_duration as f64 / constants::SLOTS_PER_YEAR as f64);

            // Calculate total compounded reward
            let total_reward = current_stake as f64 * (1.0 + apy_per_cycle).powi(total_cycles_i32)
                - current_stake as f64;

            let bump_for_vault = ctx.bumps.token_vault_account;

            let signer_seeds_for_reward: &[&[&[u8]]] =
                &[&[constants::VAULT_SEED, &[bump_for_vault]]];

            //Transfer the rewards from vault to user wallet

            let transfer_from_vault_accounts = Transfer {
                from: ctx.accounts.token_vault_account.to_account_info(),
                to: ctx.accounts.user_token_account.to_account_info(),
                authority: ctx.accounts.token_vault_account.to_account_info(),
            };

            let ctxx = CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                transfer_from_vault_accounts,
                signer_seeds_for_reward,
            );

            transfer(ctxx, total_reward as u64)?;
        }

        let staker = ctx.accounts.signer.key();
        let poolkey = ctx.accounts.pool_info.key();

        let bump_for_stake_account = ctx.bumps.stake_account;

        let signer_seeds_for_user_stake: &[&[&[u8]]] = &[&[
            constants::TOKEN_SEED,
            staker.as_ref(),
            poolkey.as_ref(),
            &[bump_for_stake_account],
        ]];

        //transfer the amount from stake account to user walllet

        let transfer_from_stake_accounts = Transfer {
            from: ctx.accounts.stake_account.to_account_info(),
            to: ctx.accounts.user_token_account.to_account_info(),
            authority: ctx.accounts.stake_account.to_account_info(),
        };

        let ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            transfer_from_stake_accounts,
            signer_seeds_for_user_stake,
        );

        transfer(ctx, stake_amount)?;

        //update the states

        stake_info.staked_amount = 0;
        stake_info.is_staked = false;
        stake_info.end_time = 0;
        stake_info.unclaimed_rewards = 0;
        stake_info.total_claimed = 0;
        stake_info.last_interaction_time = clock.slot;
        stake_info.next_claim_time = 0;

        stake_info.in_process = false;

        Ok(())
    }

    pub fn calculate_rewards(ctx: Context<Reward>, stake_counter: u64) -> Result<u64> {
        let stake_info = &mut ctx.accounts.stake_info_account;
        let pool_info = &mut ctx.accounts.pool_info;

        //Input parms Validations
        if stake_info.staked_amount <= 0 {
            return Err(ErrorCode::InvalidAmount.into());
        }

        if pool_info.apy_denominator <= 0 {
            return Err(ErrorCode::InvalidApyDenominator.into());
        }

        let reward_rate = stake_info.staked_amount * pool_info.apy
            / pool_info.apy_denominator
            / constants::SLOTS_PER_YEAR;
        let total_reward = match ctx.accounts.pool_info.roi_type {
            0 => reward_rate * constants::SLOTS_PER_DAY, // Daily reward calculation
            1 => reward_rate * constants::SLOTS_PER_WEEK, // Weekly reward calculation
            2 => reward_rate * constants::SLOTS_PER_MONTH, // Monthly reward calculation
            _ => return Err(ErrorCode::InvalidRoiType.into()), // Default case if roi_type is unhandled
        };
        msg!("Total amount: {}", total_reward);

        Ok(total_reward)
    }

    pub fn claim_rewards(ctx: Context<Reward>, stake_counter: u64) -> Result<()> {
        let stake_info = &mut ctx.accounts.stake_info_account;
        let pool_info = &mut ctx.accounts.pool_info;
        let roi_type = pool_info.roi_type;
        let clock = Clock::get()?;
        let bump_for_vault = ctx.bumps.token_vault_account;
        let signer_seeds_for_reward: &[&[&[u8]]] = &[&[constants::VAULT_SEED, &[bump_for_vault]]];

        //Ensure that the user has staked some tokens before claim
        if !stake_info.is_staked {
            return Err(ErrorCode::NotStaked.into());
        }

        //Ensure that user has not claimed all the records
        if stake_info.claim_cycles_passed == stake_info.total_claim_cycles {
            return Err(ErrorCode::AlreadyClaimed.into());
        }

        //Ensure that the claim time is not over
        if stake_info.last_interaction_time > stake_info.end_time {
            return Err(ErrorCode::TimeOver.into());
        }

        //Ensure that the rewards can be only claimed if autostake is false
        if stake_info.autostake {
            return Err(ErrorCode::NoClaim.into());
        }

        //Ensure non re-entrance
        if stake_info.in_process {
            return Err(ErrorCode::AlreadyInProcess.into());
        }

        stake_info.in_process = true;

        let reward_rate = stake_info.staked_amount * pool_info.apy
            / pool_info.apy_denominator
            / constants::SLOTS_PER_YEAR;

        let (reward_cycle_length, reward_rate_for_cycle) = match roi_type {
            0 => (
                constants::SLOTS_PER_DAY,
                reward_rate * constants::SLOTS_PER_DAY,
            ), // Daily reward
            1 => (
                constants::SLOTS_PER_WEEK,
                reward_rate * constants::SLOTS_PER_WEEK,
            ), // Weekly reward
            2 => (
                constants::SLOTS_PER_MONTH,
                reward_rate * constants::SLOTS_PER_MONTH,
            ), // Monthly reward
            _ => return Err(ErrorCode::InvalidRoiType.into()),
        };

        // Calculate how many reward cycles have passed
        let max_cycles = stake_info.total_claim_cycles;
        let claimed_cycles = stake_info.claim_cycles_passed; // Already claimed cycles
        let cycles_passed = (clock.slot - stake_info.last_interaction_time) / reward_cycle_length;
        // Calculate how many additional cycles can be claimed
        let remaining_cycles = (cycles_passed as u64).min(max_cycles - claimed_cycles);

        if cycles_passed < 1 {
            return Err(ErrorCode::Wait.into()); // Not enough time passed for any reward cycle
        }

        // Calculate the total reward for missed cycles
        let total_claimable_rewards =
            stake_info.unclaimed_rewards + (reward_rate_for_cycle * remaining_cycles as u64);

        if total_claimable_rewards <= 0 {
            // There are not enough claimable rewards
            return Err(ErrorCode::NoReward.into());
        }

        // Transfer the total reward to the user
        let transfer_from_vault_accounts = Transfer {
            from: ctx.accounts.token_vault_account.to_account_info(),
            to: ctx.accounts.user_token_account.to_account_info(),
            authority: ctx.accounts.token_vault_account.to_account_info(),
        };

        let ctxx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            transfer_from_vault_accounts,
            signer_seeds_for_reward,
        );

        transfer(ctxx, total_claimable_rewards)?;

        // Reset unclaimed rewards and update claim time
        stake_info.total_claimed += total_claimable_rewards;
        if stake_info.unclaimed_rewards >= total_claimable_rewards {
            stake_info.unclaimed_rewards = stake_info.unclaimed_rewards - total_claimable_rewards;
        } else {
            stake_info.unclaimed_rewards = 0;
        }
        stake_info.next_claim_time = stake_info.last_interaction_time
            + (reward_cycle_length * ((remaining_cycles as u64) + 1)); // Move to the next claim period
        stake_info.last_interaction_time = clock.slot;
        stake_info.claim_cycles_passed += remaining_cycles;
        stake_info.in_process = false;
        Ok(())
    }

    pub fn restake_rewards(ctx: Context<Reward>, stake_counter: u64) -> Result<()> {
        let stake_info = &mut ctx.accounts.stake_info_account;
        let pool_info = &mut ctx.accounts.pool_info;
        let roi_type = pool_info.roi_type;
        let clock = Clock::get()?;
        let bump_for_vault = ctx.bumps.token_vault_account;
        let signer_seeds_for_reward: &[&[&[u8]]] = &[&[constants::VAULT_SEED, &[bump_for_vault]]];

        //Ensure that the user has staked some tokens before claim
        if !stake_info.is_staked {
            return Err(ErrorCode::NotStaked.into());
        }

        //Ensure that user has not claimed all the records
        if stake_info.claim_cycles_passed == stake_info.total_claim_cycles {
            return Err(ErrorCode::AlreadyClaimed.into());
        }

        //Ensure that the claim time is not over
        if stake_info.last_interaction_time > stake_info.end_time {
            return Err(ErrorCode::TimeOver.into());
        }

        //Ensure that the rewards can be only claimed if autostake is false
        if stake_info.autostake {
            return Err(ErrorCode::NoClaim.into());
        }

        //Ensure non re-entrance
        if stake_info.in_process {
            return Err(ErrorCode::AlreadyInProcess.into());
        }

        stake_info.in_process = true;

        let reward_rate = stake_info.staked_amount * pool_info.apy
            / pool_info.apy_denominator
            / constants::SLOTS_PER_YEAR;

        let (reward_cycle_length, reward_rate_for_cycle) = match roi_type {
            0 => (
                constants::SLOTS_PER_DAY,
                reward_rate * constants::SLOTS_PER_DAY,
            ), // Daily reward
            1 => (
                constants::SLOTS_PER_WEEK,
                reward_rate * constants::SLOTS_PER_WEEK,
            ), // Weekly reward
            2 => (
                constants::SLOTS_PER_MONTH,
                reward_rate * constants::SLOTS_PER_MONTH,
            ), // Monthly reward
            _ => return Err(ErrorCode::InvalidRoiType.into()),
        };

        // Calculate how many reward cycles have passed
        let max_cycles = stake_info.total_claim_cycles;
        let claimed_cycles = stake_info.claim_cycles_passed; // Already claimed cycles
        let cycles_passed = (clock.slot - stake_info.last_interaction_time) / reward_cycle_length;
        // Calculate how many additional cycles can be claimed
        let remaining_cycles = (cycles_passed as u64).min(max_cycles - claimed_cycles);

        if cycles_passed < 1 {
            return Err(ErrorCode::Wait.into()); // Not enough time passed for any reward cycle
        }

        // Calculate the total reward for missed cycles
        let total_claimable_rewards =
            stake_info.unclaimed_rewards + (reward_rate_for_cycle * remaining_cycles as u64);

        if total_claimable_rewards <= 0 {
            return Err(ErrorCode::NoReward.into());
        }

        // Transfer the total reward to the user
        let transfer_from_vault_accounts = Transfer {
            from: ctx.accounts.token_vault_account.to_account_info(),
            to: ctx.accounts.stake_account.to_account_info(),
            authority: ctx.accounts.token_vault_account.to_account_info(),
        };

        let ctxx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            transfer_from_vault_accounts,
            signer_seeds_for_reward,
        );

        transfer(ctxx, total_claimable_rewards)?;

        // Reset unclaimed rewards and update claim time
        stake_info.staked_amount += total_claimable_rewards;
        stake_info.total_claimed += total_claimable_rewards;
        if stake_info.unclaimed_rewards >= total_claimable_rewards {
            stake_info.unclaimed_rewards = stake_info.unclaimed_rewards - total_claimable_rewards;
        } else {
            stake_info.unclaimed_rewards = 0;
        }
        stake_info.next_claim_time = stake_info.last_interaction_time
            + (reward_cycle_length * ((remaining_cycles as u64) + 1)); // Move to the next claim period
        stake_info.last_interaction_time = clock.slot;
        stake_info.claim_cycles_passed += remaining_cycles;

        stake_info.in_process = false;

        Ok(())
    }

    pub fn update_pool_info(
        ctx: Context<UpdatePoolInfo>,
        admin: Pubkey,
        token_vault: Pubkey,
        lock_time: u64,
        apy: u64,
        apy_denominator: u64,
        roi_type: u64,
        token: Pubkey,
    ) -> Result<()> {
        // Only the current admin (owner) can update the pool_info
        if ctx.accounts.admin.key() != ctx.accounts.pool_info.admin {
            return Err(ErrorCode::Unauthorized.into());
        }

        // Update the pool info
        let pool_info = &mut ctx.accounts.pool_info;
        pool_info.admin = admin;
        pool_info.token_vault = token_vault;
        pool_info.lock_time = lock_time;
        pool_info.apy = apy;
        pool_info.apy_denominator = apy_denominator;
        pool_info.roi_type = roi_type;
        pool_info.token = token;

        Ok(())
    }

    pub fn admin_withdraw(ctx: Context<AdminWithdraw>, value: u64) -> Result<()> {
        // Only the current admin (owner) can withdraw from Treasury
        if ctx.accounts.signer.key() != ctx.accounts.pool_info.admin {
            return Err(ErrorCode::Unauthorized.into());
        }
        let bump_for_vault = ctx.bumps.token_vault_account;

        let signer_seeds_for_reward: &[&[&[u8]]] = &[&[constants::VAULT_SEED, &[bump_for_vault]]];

        let transfer_from_vault_accounts = Transfer {
            from: ctx.accounts.token_vault_account.to_account_info(),
            to: ctx.accounts.admin_token_account.to_account_info(),
            authority: ctx.accounts.token_vault_account.to_account_info(),
        };

        let ctxx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            transfer_from_vault_accounts,
            signer_seeds_for_reward,
        );

        transfer(ctxx, value)?;

        Ok(())
    }

}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut,address=pubkey!("CCbVpAQrdVhFoteeHj6mA7oeumnvzk36sMm7TfGaCEkD"))]
    pub signer: Signer<'info>,
    #[account(mut)]
    pub admin: UncheckedAccount<'info>,
    #[account(
        init_if_needed,
        seeds = [constants::VAULT_SEED],
        bump,
        payer = signer,
        token::mint = mint ,
        token::authority= token_vault_account,
    )]
    pub token_vault_account: Account<'info, TokenAccount>,
    #[account(init, payer = signer, space = 8 + std::mem::size_of::<PoolInfo>())]
    pub pool_info: Account<'info, PoolInfo>,
    pub mint: Account<'info, Mint>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}
#[account]
pub struct PoolInfo {
    pub admin: Pubkey,
    pub token_vault: Pubkey,
    pub lock_time: u64,
    pub apy: u64,
    pub apy_denominator: u64,
    pub roi_type: u64,
    pub token: Pubkey,
}

#[derive(Accounts)]
#[instruction(stake_counter: u64)]
pub struct Stake<'info> {
    #[account(mut)]
    pub signer: Signer<'info>,

    #[account(
        init_if_needed,
        seeds = [ &stake_counter.to_le_bytes().as_ref(), constants::STAKE_INFO_SEED, signer.key.as_ref(), pool_info.key().as_ref(),  ],
        bump,
        payer = signer, 
        space = 8 + 8 + std::mem::size_of::<StakeInfo>(),
    )]
    pub stake_info_account: Account<'info, StakeInfo>,

    #[account(
        init_if_needed,
        seeds = [constants::TOKEN_SEED, signer.key.as_ref(), pool_info.key().as_ref()],
        bump,
        payer = signer,
        token::mint = mint,
        token::authority = stake_account
    )]
    pub stake_account: Account<'info, TokenAccount>,
    pub pool_info: Account<'info, PoolInfo>,

    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = signer,
    )]
    pub user_token_account: Account<'info, TokenAccount>,

    pub mint: Account<'info, Mint>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(stake_counter: u64)]
pub struct DeStake<'info> {
    #[account(mut)]
    pub signer: Signer<'info>,

    #[account(
        mut,
        seeds = [constants::VAULT_SEED],
        bump,
    )]
    pub token_vault_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        seeds = [ &stake_counter.to_le_bytes().as_ref(), constants::STAKE_INFO_SEED, signer.key.as_ref(), pool_info.key().as_ref(),  ],
        bump,
    )]
    pub stake_info_account: Account<'info, StakeInfo>,
    pub pool_info: Account<'info, PoolInfo>,

    #[account(
        mut,
        seeds = [constants::TOKEN_SEED, signer.key.as_ref(), pool_info.key().as_ref()],
        bump,
    )]
    pub stake_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = signer,
    )]
    pub user_token_account: Account<'info, TokenAccount>,

    pub mint: Account<'info, Mint>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(stake_counter: u64)]
pub struct Reward<'info> {
    #[account(mut)]
    pub signer: Signer<'info>,
    #[account(
        mut,
        seeds = [ &stake_counter.to_le_bytes().as_ref(), constants::STAKE_INFO_SEED, signer.key.as_ref(), pool_info.key().as_ref(),  ],
        bump,
    )]
    pub stake_info_account: Account<'info, StakeInfo>,
    pub pool_info: Account<'info, PoolInfo>,

    #[account(
        mut,
        seeds = [constants::TOKEN_SEED, signer.key.as_ref(), pool_info.key().as_ref()],
        bump,
    )]
    pub stake_account: Account<'info, TokenAccount>,
    #[account(
        mut,
        seeds = [constants::VAULT_SEED],
        bump,
    )]
    pub token_vault_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = signer,
    )]
    pub user_token_account: Account<'info, TokenAccount>,
    pub mint: Account<'info, Mint>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct UpdatePoolInfo<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,
    #[account(mut)]
    pub pool_info: Account<'info, PoolInfo>,
}

#[derive(Accounts)]
pub struct AdminWithdraw<'info> {
    #[account(mut)]
    pub signer: Signer<'info>,

    #[account(
        mut,
        seeds = [constants::VAULT_SEED],
        bump,
    )]
    pub token_vault_account: Account<'info, TokenAccount>,

    pub pool_info: Account<'info, PoolInfo>,

    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = signer,
    )]
    pub admin_token_account: Account<'info, TokenAccount>,

    pub mint: Account<'info, Mint>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[account]
pub struct StakeInfo {
    pub staked_amount: u64,
    pub deposit_timestamp: i64,
    pub stake_at_slot: u64,
    pub is_staked: bool,
    pub end_time: u64,
    pub autostake: bool,
    pub unclaimed_rewards: u64,
    pub last_interaction_time: u64,
    pub next_claim_time: u64,
    pub pool_info: Pubkey,
    pub total_claimed: u64,
    pub total_claim_cycles: u64,
    pub claim_cycles_passed: u64,
    pub stake_seed: u64,
    pub in_process: bool,
}

#[error_code]
pub enum ErrorCode {
    #[msg("Token are already staked")]
    IsStaked,
    #[msg("Tokens not staked")]
    NotStaked,
    #[msg("No tokens to stake")]
    NoTokens,
    #[msg("Invalid ROI type: must be 0, 1, or 2.")]
    InvalidRoiType,
    #[msg("You don't have any rewards to claim.")]
    NoReward,
    #[msg("Too Early: You need to wait for claim cycle.")]
    Wait,
    #[msg("Your auto stake feature is enabled, you can't claim periodic rewards")]
    NoClaim,
    #[msg("You can not destake before the Lock Period is over")]
    StillLocked,
    #[msg("You are not authorized to call this function")]
    Unauthorized,
    #[msg("Claim Time is over, You can not claim now")]
    TimeOver,
    #[msg("All the rewards have already been Claimed.")]
    AlreadyClaimed,
    #[msg("Invalid APY denominator: cannot be zero.")]
    InvalidApyDenominator,
    #[msg("Invalid APY %: cannot be zero.")]
    InvalidApy,
    #[msg("Invalid lock period: cannot be zero.")]
    InvalidLockTime,
    #[msg("Unauthorized admin address.")]
    UnauthorizedAdmin,
    #[msg("Stake Seed Already in use.")]
    IsStakeSeed,
    #[msg("Invalid Stake Amount.")]
    InvalidAmount,
    #[msg("Please Wait, Another Transaction is already Processing")]
    AlreadyInProcess,
    #[msg("Cycle count exceed i32 range")]
    InvalidCycleCount,
}
