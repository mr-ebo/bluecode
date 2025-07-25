use sqlx::PgPool;
use time::PrimitiveDateTime;
use uuid::Uuid;

/// Module and schema representing a refund.
///
/// A refund is always tied to a specific payment record, but it is possible
/// to make partial refunds (i.e. refund less than the total payment amount).
/// In the same vein, it is possible to apply several refunds against the same
/// payment record, the but sum of all refunded amounts for a given payment can
/// never surpass the original payment amount.
///
/// If a refund is persisted in the database, it is considered effective: the
/// bank's client will have the money credited to their account.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Refund {
    pub id: Uuid,
    pub payment_id: Uuid,
    pub amount: i32,
    pub inserted_at: PrimitiveDateTime,
    pub updated_at: PrimitiveDateTime,
}

#[derive(Debug)]
pub enum CreateError {
    PaymentNotFound,
    ExcessiveAmount,
    Database(sqlx::Error),
}

pub async fn create(pool: &PgPool, payment_id: Uuid, amount: i32) -> Result<Refund, CreateError> {
    let mut transaction = pool.begin().await.map_err(CreateError::Database)?;
    let refund = sqlx::query_as!(
        Refund,
        r#"
               INSERT INTO refunds ( id, payment_id, amount, inserted_at, updated_at )
               VALUES ( $1, $2, $3, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP )
            RETURNING id, payment_id, amount, inserted_at, updated_at
        "#,
        Uuid::new_v4(),
        payment_id,
        amount,
    )
    .fetch_one(&mut transaction)
    .await
    .map_err(|e| {
        let err = e.as_database_error().unwrap();
        // postgresql error codes: https://www.postgresql.org/docs/current/errcodes-appendix.html
        // 23503 = foreign_key_violation
        if err.code().unwrap() == "23503" && err.constraint() == Some("refunds_payment_id_fkey") {
            CreateError::PaymentNotFound
        } else {
            CreateError::Database(e)
        }
    })?;

    let count = sqlx::query!(
        r#"
            UPDATE payments
               SET refunded_amount = refunded_amount + $1
             WHERE id = $2
               AND status = 'Approved'
               AND refunded_amount + $1 <= amount
        "#,
        amount,
        payment_id
    )
    .execute(&mut transaction)
    .await
    .map_err(CreateError::Database)?;

    if count.rows_affected() == 0 {
        return Err(CreateError::ExcessiveAmount);
    }

    transaction.commit().await.map_err(CreateError::Database)?;

    Ok(refund)
}

pub async fn get(pool: &PgPool, id: Uuid) -> Result<Refund, sqlx::Error> {
    sqlx::query_as!(
        Refund,
        r#"
            SELECT id, payment_id, amount, inserted_at, updated_at FROM refunds
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
    use crate::bank::payments::Payment;

    pub const REFUND_AMOUNT: i32 = 42;

    impl Refund {
        pub async fn new_test(pool: &PgPool) -> Result<Refund, sqlx::Error> {
            let payment = Payment::new_test(pool).await?;

            let refund = create(pool, payment.id, REFUND_AMOUNT)
                .await
                .map_err(|e| match e {
                    CreateError::Database(err) => err,
                    _ => panic!("Not a database error: {:?}", e),
                })?;

            get(pool, refund.id).await
        }
    }

    #[tokio::test]
    async fn test_refund() {
        let pool = crate::pg_pool()
            .await
            .expect("failed to connect to postgres");

        let refund = Refund::new_test(&pool)
            .await
            .expect("failed to create refund");

        assert_eq!(refund.amount, REFUND_AMOUNT);
    }
}
