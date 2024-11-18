use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};
declare_id!("GJdNPUnhyjz4x47fibedUxmLY7Xx42E6SrQGfEJUzj9S");

pub mod constants {
    pub const ESCROW_SEED: &[u8] = b"vault";
    pub const DATA_SEED: &[u8] = b"data_account";
}

#[program]
pub mod token_claim_program {

    use super::*;

    pub fn initialize(
        ctx: Context<Initialize>,
        round: u8,
        claim_type: u8,
        batch: u8,
        _list_size: u64,
        beneficiaries: Vec<Beneficiary>,
        amount: u64,
        decimals: u8,
    ) -> Result<()> {
        let data_account = &mut ctx.accounts.data_account;
        data_account.beneficiaries = beneficiaries;

        if data_account.batch == batch {
            return Err(ErrorCode::IsBatched.into());
        }

        data_account.released = false;
        data_account.round = round;
        data_account.claim_type = claim_type;
        data_account.batch = batch;
        data_account.token_amount = amount;
        data_account.decimals = decimals; // b/c bpf does not have any floats
        data_account.initializer = ctx.accounts.sender.to_account_info().key();
        data_account.escrow_wallet = ctx.accounts.escrow_wallet.to_account_info().key();
        data_account.token_mint = ctx.accounts.token_mint.to_account_info().key();

        let transfer_instruction = Transfer {
            from: ctx.accounts.wallet_to_withdraw_from.to_account_info(),
            to: ctx.accounts.escrow_wallet.to_account_info(),
            authority: ctx.accounts.sender.to_account_info(),
        };

        let cpi_ctx = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            transfer_instruction,
        );

        token::transfer(
            cpi_ctx,
            data_account.token_amount * u64::pow(10, decimals as u32),
        )?;

        Ok(())
    }

    pub fn release(
        ctx: Context<Release>,
        _round: u8,
        _claim_type: u8,
        _batch: u8,
        released: bool,
    ) -> Result<()> {
        let data_account = &mut ctx.accounts.data_account;

        data_account.released = released;
        Ok(())
    }

    pub fn update_user_status(
        ctx: Context<UpdateUser>,
        _round: u8,
        _claim_type: u8,
        _batch: u8,
        user_wallet: Pubkey,
        blocked: bool,
    ) -> Result<()> {
        let data_account = &mut ctx.accounts.data_account;
        let beneficiaries = &data_account.beneficiaries;

        let (index, _beneficiary) = beneficiaries
            .iter()
            .enumerate()
            .find(|(_, _beneficiary)| _beneficiary.key == user_wallet)
            .ok_or(ErrorCode::BeneficiaryNotFound)?;

        // beneficiary.is_blocked = blocked;
        data_account.beneficiaries[index].is_blocked = blocked;

        Ok(())
    }

    pub fn update_bulk_user_status(
        ctx: Context<UpdateUser>,
        _round: u8,
        _claim_type: u8,
        _batch: u8,
        blocked: bool,
    ) -> Result<()> {
        let data_account = &mut ctx.accounts.data_account;

        // Iterate through the beneficiaries and update the status
        for beneficiary in &mut data_account.beneficiaries {
            if !beneficiary.is_claimed {
                beneficiary.is_blocked = blocked;
            }
        }

        Ok(())
    }


    pub fn withdraw_from_escrow(
        ctx: Context<Withdraw>,
        _round: u8,
        _claim_type: u8,
        _batch: u8,
    ) -> Result<()> {
        let escrow_wallet = &mut ctx.accounts.escrow_wallet;
        let data_account = &mut ctx.accounts.data_account;
        let beneficiaries = &data_account.beneficiaries;
        let token_program = &mut ctx.accounts.token_program;
        let token_mint_key = &mut ctx.accounts.token_mint.key();
        let admin_ata = &mut ctx.accounts.wallet_to_deposit_to;
        let decimals = data_account.decimals;

        let amount_to_withdraw: u64 = beneficiaries
            .iter()
            .filter(|beneficiary| beneficiary.is_blocked) // Filter only blocked beneficiaries
            .map(|beneficiary| beneficiary.allocated_tokens) // Map to allocated tokens
            .sum(); // Sum them up

        let bump_for_data = ctx.bumps.escrow_wallet;

        let data_account_key = data_account.key();

        // Transfer Logic:
        let seeds: &[&[&[u8]]] = &[&[
            constants::ESCROW_SEED,
            token_mint_key.as_ref(),
            data_account_key.as_ref(),
            &[bump_for_data],
        ]];

        let transfer_instruction = Transfer {
            from: escrow_wallet.to_account_info(),
            to: admin_ata.to_account_info(),
            authority: escrow_wallet.to_account_info(),
        };

        let cpi_ctx = CpiContext::new_with_signer(
            token_program.to_account_info(),
            transfer_instruction,
            seeds,
        );

        token::transfer(cpi_ctx, amount_to_withdraw * u64::pow(10, decimals as u32))?;

        Ok(())
    }

    pub fn claim(ctx: Context<Claim>, _round: u8, _claim_type: u8, _batch: u8) -> Result<()> {
        let sender = &mut ctx.accounts.sender;
        let escrow_wallet = &mut ctx.accounts.escrow_wallet;
        let data_account = &mut ctx.accounts.data_account;
        let beneficiaries = &data_account.beneficiaries;
        let token_program = &mut ctx.accounts.token_program;
        let token_mint_key = &mut ctx.accounts.token_mint.key();
        let beneficiary_ata = &mut ctx.accounts.wallet_to_deposit_to;
        let decimals = data_account.decimals;

        let (index, beneficiary) = beneficiaries
            .iter()
            .enumerate()
            .find(|(_, beneficiary)| beneficiary.key == *sender.to_account_info().key)
            .ok_or(ErrorCode::BeneficiaryNotFound)?;

        let amount_to_transfer = beneficiary.allocated_tokens;

        require!(beneficiary.in_process == false, ErrorCode::ClaimNotAllowed);
        require!(beneficiary.is_claimed == false, ErrorCode::ClaimNotAllowed);
        require!(beneficiary.is_blocked == false, ErrorCode::ClaimNotAllowed);

        data_account.beneficiaries[index].in_process = true;
        let bump_for_data = ctx.bumps.escrow_wallet;

        let data_account_key = data_account.key();

        // Transfer Logic:
        let seeds: &[&[&[u8]]] = &[&[
            constants::ESCROW_SEED,
            token_mint_key.as_ref(),
            data_account_key.as_ref(),
            &[bump_for_data],
        ]];

        let transfer_instruction = Transfer {
            from: escrow_wallet.to_account_info(),
            to: beneficiary_ata.to_account_info(),
            authority: escrow_wallet.to_account_info(),
        };

        let cpi_ctx = CpiContext::new_with_signer(
            token_program.to_account_info(),
            transfer_instruction,
            seeds,
        );

        token::transfer(cpi_ctx, amount_to_transfer * u64::pow(10, decimals as u32))?;
        data_account.beneficiaries[index].is_claimed = true;
        data_account.beneficiaries[index].in_process = false;

        Ok(())
    }
}

#[derive(Accounts)]
#[instruction(round: u8 , claim_type: u8, batch: u8,list_size: u64)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = sender,
        space = 8 + 1 + 1 + 1 + 1 + 8 + 32 + 32 + 32 + 1 + (4 +( list_size as usize * (32 + 8 + 1 + 1 + 1)) + 1), // define the size
        seeds = [&round.to_le_bytes().as_ref(), &claim_type.to_le_bytes().as_ref(), &batch.to_le_bytes().as_ref(), constants::DATA_SEED, token_mint.key().as_ref()], 
        bump 
    )]
    pub data_account: Account<'info, DataAccount>,

    #[account(
        init,
        payer = sender,
        seeds=[constants::ESCROW_SEED.as_ref(), token_mint.key().as_ref(), data_account.key().as_ref()],
        bump,
        token::mint=token_mint,
        token::authority=escrow_wallet,
    )]
    pub escrow_wallet: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint=wallet_to_withdraw_from.owner == sender.key(),
        constraint=wallet_to_withdraw_from.mint == token_mint.key()
    )]
    pub wallet_to_withdraw_from: Account<'info, TokenAccount>,

    pub token_mint: Account<'info, Mint>,

    #[account(mut)]
    pub sender: Signer<'info>,

    pub system_program: Program<'info, System>,

    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
#[instruction(round: u8 , claim_type: u8, batch: u8)]
pub struct Release<'info> {
    #[account(
        mut,
        seeds = [&round.to_le_bytes().as_ref(), &claim_type.to_le_bytes().as_ref(), &batch.to_le_bytes().as_ref(), constants::DATA_SEED, token_mint.key().as_ref()],
        bump,
        constraint=data_account.initializer == sender.key() @ ErrorCode::InvalidSender
    )]
    pub data_account: Account<'info, DataAccount>,

    pub token_mint: Account<'info, Mint>,

    #[account(mut)]
    pub sender: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(round: u8 , claim_type: u8, batch: u8)]
pub struct UpdateUser<'info> {
    #[account(
        mut,
        seeds = [&round.to_le_bytes().as_ref(), &claim_type.to_le_bytes().as_ref(), &batch.to_le_bytes().as_ref(), constants::DATA_SEED, token_mint.key().as_ref()],
        bump,
        constraint=data_account.initializer == sender.key() @ ErrorCode::InvalidSender
    )]
    pub data_account: Account<'info, DataAccount>,

    pub token_mint: Account<'info, Mint>,

    #[account(mut)]
    pub sender: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(round: u8 , claim_type: u8, batch: u8)]
pub struct Claim<'info> {
    #[account(
        mut,
        seeds = [&round.to_le_bytes().as_ref(), &claim_type.to_le_bytes().as_ref(), &batch.to_le_bytes().as_ref(), constants::DATA_SEED, token_mint.key().as_ref()],
        bump,
    )]
    pub data_account: Account<'info, DataAccount>,

    #[account(
        mut,
       seeds=[constants::ESCROW_SEED.as_ref(), token_mint.key().as_ref(), data_account.key().as_ref()],
        bump,
    )]
    pub escrow_wallet: Account<'info, TokenAccount>,

    #[account(mut)]
    pub sender: Signer<'info>,

    pub token_mint: Account<'info, Mint>,

    #[account(
        init_if_needed,
        payer = sender,
        associated_token::mint = token_mint,
        associated_token::authority = sender,
    )]
    pub wallet_to_deposit_to: Account<'info, TokenAccount>,

    pub associated_token_program: Program<'info, AssociatedToken>,

    pub token_program: Program<'info, Token>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(round: u8 , claim_type: u8, batch: u8)]
pub struct Withdraw<'info> {
    #[account(
        mut,
        seeds = [&round.to_le_bytes().as_ref(), &claim_type.to_le_bytes().as_ref(), &batch.to_le_bytes().as_ref(), constants::DATA_SEED, token_mint.key().as_ref()],
        bump,
        constraint=data_account.initializer == sender.key() @ ErrorCode::InvalidSender
    )]
    pub data_account: Account<'info, DataAccount>,

    #[account(
        mut,
       seeds=[constants::ESCROW_SEED.as_ref(), token_mint.key().as_ref(), data_account.key().as_ref()],
        bump,
    )]
    pub escrow_wallet: Account<'info, TokenAccount>,

    #[account(mut)]
    pub sender: Signer<'info>,

    pub token_mint: Account<'info, Mint>,

    #[account(
        init_if_needed,
        payer = sender,
        associated_token::mint = token_mint,
        associated_token::authority = sender,
    )]
    pub wallet_to_deposit_to: Account<'info, TokenAccount>,

    pub associated_token_program: Program<'info, AssociatedToken>,

    pub token_program: Program<'info, Token>,

    pub system_program: Program<'info, System>,
}

#[derive(Default, Copy, Clone, AnchorSerialize, AnchorDeserialize)]
pub struct Beneficiary {
    pub key: Pubkey,           // 32
    pub allocated_tokens: u64, // 8
    pub is_claimed: bool,      //1
    pub is_blocked: bool,      //1
    pub in_process: bool,      //1    // to avoid race condition
}

#[account]
#[derive(Default)]
pub struct DataAccount {
    // Space in bytes: 8 + 1 + 1 + 1 + 8 + 32 + 32 + 32 + 1 + (4 + (100 * (32 + 8 + 8)))
    pub released: bool,                  // 1
    pub round: u8,                       //1
    pub claim_type: u8, //1             //0-> IDO, 1-> SAFT, 2-> Tokensoft Presale, 3-> Utherverse Presale, 4-> Contest, 5-> Investors
    pub batch: u8,      //1
    pub token_amount: u64, // 8
    pub initializer: Pubkey, // 32
    pub escrow_wallet: Pubkey, // 32
    pub token_mint: Pubkey, // 32
    pub beneficiaries: Vec<Beneficiary>, // (4 + (n * (32 + 8 + 8)))
    pub decimals: u8,   // 1
}

#[error_code]
pub enum ErrorCode {
    #[msg("Sender is not owner of Data Account")]
    InvalidSender,
    #[msg("Not allowed to claim new tokens currently")]
    ClaimNotAllowed,
    #[msg("Beneficiary does not exist in account")]
    BeneficiaryNotFound,
    #[msg("Batch already exist")]
    IsBatched,
}
