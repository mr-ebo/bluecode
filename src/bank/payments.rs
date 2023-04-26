use crate::bank::accounts::{AccountService, HoldRef};
use lazy_static::lazy_static;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::str::FromStr;
use strum_macros::EnumString;
use time::PrimitiveDateTime;
use uuid::Uuid;

lazy_static! {
    static ref CARD_NUMBER_REGEX: Regex = Regex::new(r"^\d{15}$").unwrap();
}

#[derive(Debug, Clone, PartialEq, Eq, Copy, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// The payment is being processed, and it's state is unknown.
    Processing,
    /// The payment was approved by the bank.
    Approved,
    /// The payment was declined by the bank (e.g. insufficient funds).
    Declined,
    /// The payment was unable to complete (e.g. banking system crashed).
    Failed,
}

#[derive(Debug)]
pub enum InvalidArgumentError {
    NegativeAmount,
    ZeroAmount,
    InvalidCardFormat,
}

#[derive(Debug, Eq, PartialEq, EnumString)]
#[strum(serialize_all = "snake_case")]
pub enum AccountServiceError {
    InsufficientFunds,
    InvalidAccountNumber,
    ServiceUnavailable,
    InternalError,
}

#[derive(Debug)]
pub enum CreateError {
    DuplicatedCardNumber,
    InvalidArgument(InvalidArgumentError),
    AccountService(AccountServiceError),
    Database(sqlx::Error),
}

// Struct representing a payment.
//
// Once a payment has been persisted with an "approved" state, the merchant is guaranteed to
// receive money from the bank: they can therefore release the purchased goods to the customer.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct Payment {
    pub id: Uuid,
    pub amount: i32,
    pub refunded_amount: i32,
    pub card_number: String,
    pub status: Status,
    pub inserted_at: PrimitiveDateTime,
    pub updated_at: PrimitiveDateTime,
}

async fn insert(
    pool: &PgPool,
    amount: i32,
    card_number: &str,
    status: Status,
) -> Result<Payment, sqlx::Error> {
    sqlx::query_as!(
        Payment,
        r#"
               INSERT INTO payments ( id, amount, card_number, status, inserted_at, updated_at )
               VALUES ( $1, $2, $3, $4, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP )
            RETURNING id, amount, refunded_amount, card_number, inserted_at, updated_at, status as "status: _"
        "#,
        Uuid::new_v4(),
        amount,
        card_number.to_string(),
        status as Status
    )
    .fetch_one(pool)
    .await
}

async fn hold_account(
    account_service: &impl AccountService,
    card_number: &str,
    amount: i32,
) -> Result<HoldRef, AccountServiceError> {
    account_service
        .place_hold(card_number, amount)
        .await
        .map_err(|msg| AccountServiceError::from_str(msg.as_str()).unwrap())
}

async fn validate_payment_inputs(
    amount: i32,
    card_number: &str,
) -> Result<(), InvalidArgumentError> {
    if amount < 0 {
        Err(InvalidArgumentError::NegativeAmount)
    } else if amount == 0 {
        Err(InvalidArgumentError::ZeroAmount)
    } else if !CARD_NUMBER_REGEX.is_match(card_number) {
        Err(InvalidArgumentError::InvalidCardFormat)
    } else {
        Ok(())
    }
}

pub async fn create(
    pool: &PgPool,
    account_service: &impl AccountService,
    amount: i32,
    card_number: &str,
    status: Status,
) -> Result<Payment, CreateError> {
    validate_payment_inputs(amount, card_number)
        .await
        .map_err(CreateError::InvalidArgument)?;
    let _ = hold_account(account_service, card_number, amount)
        .await
        .map_err(CreateError::AccountService)?;
    insert(pool, amount, card_number, status)
        .await
        // TODO: call account_service.release_hold(hold_ref)
        .map_err(|e| {
            let err = e.as_database_error().unwrap();
            if err.code().unwrap() == "23505"
                && err.constraint() == Some("payments_card_number_index")
            {
                CreateError::DuplicatedCardNumber
            } else {
                CreateError::Database(e)
            }
        })
}

pub async fn get(pool: &PgPool, id: Uuid) -> Result<Payment, sqlx::Error> {
    sqlx::query_as!(
        Payment,
        r#"
            SELECT id, amount, refunded_amount, card_number, inserted_at, updated_at, status as "status: _"
              FROM payments
             WHERE id = $1
        "#,
        id
    )
    .fetch_one(pool)
    .await
}

#[cfg(test)]
pub mod tests {

    use super::*;
    use crate::bank::payment_instruments::Card;

    pub const PAYMENT_AMOUNT: i32 = 1_23;
    pub const PAYMENT_STATUS: Status = Status::Approved;

    impl Payment {
        pub async fn new_test(pool: &PgPool) -> Result<Payment, sqlx::Error> {
            let card_number: String = Card::new_test().into();

            insert(pool, PAYMENT_AMOUNT, card_number.as_str(), PAYMENT_STATUS).await
        }
    }

    #[tokio::test]
    async fn test_payment() {
        let pool = crate::pg_pool()
            .await
            .expect("failed to connect to postgres");

        let payment = Payment::new_test(&pool)
            .await
            .expect("failed to create payment");

        assert_eq!(payment.amount, PAYMENT_AMOUNT);
        assert_eq!(payment.status, PAYMENT_STATUS);
    }
}
