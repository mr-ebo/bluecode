use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::BankWeb;
use crate::bank::{accounts::AccountService, payments};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct RequestData {
    pub amount: i32,
    pub card_number: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct RequestBody {
    pub payment: RequestData,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ResponseData {
    pub id: Uuid,
    pub amount: i32,
    pub card_number: String,
    pub status: payments::Status,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ResponseBody {
    pub data: ResponseData,
}

pub async fn post<T: AccountService>(
    State(bank_web): State<BankWeb<T>>,
    Json(body): Json<RequestBody>,
) -> (StatusCode, Json<ResponseBody>) {
    let payment_id = payments::insert(
        &bank_web.pool,
        body.payment.amount,
        body.payment.card_number,
        payments::Status::Approved,
    )
    .await
    .unwrap();

    let payment = payments::get(&bank_web.pool, payment_id).await.unwrap();

    (
        StatusCode::CREATED,
        Json(ResponseBody {
            data: ResponseData {
                id: payment.id,
                amount: payment.amount,
                card_number: payment.card_number,
                status: payment.status,
            },
        }),
    )
}

pub async fn get<T: AccountService>(
    State(bank_web): State<BankWeb<T>>,
    Path(payment_id): Path<Uuid>,
) -> (StatusCode, Json<ResponseBody>) {
    let payment = payments::get(&bank_web.pool, payment_id).await.unwrap();

    (
        StatusCode::OK,
        Json(ResponseBody {
            data: ResponseData {
                id: payment.id,
                amount: payment.amount,
                card_number: payment.card_number,
                status: payment.status,
            },
        }),
    )
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::{
        bank::{payment_instruments::Card, payments::Status},
        bank_web::tests::{deserialize_response_body, post},
    };
    use axum::Router;

    async fn do_payment(
        router: &Router,
        payment_amount: i32,
        payment_card_number: String,
        expected_status_code: StatusCode,
        expected_status: Status,
    ) {
        let request_body = RequestBody {
            payment: RequestData {
                amount: payment_amount,
                card_number: payment_card_number,
            },
        };

        let response = post(router, "/api/payments", &request_body).await;
        assert_eq!(response.status(), expected_status_code);

        let response_body = deserialize_response_body::<ResponseBody>(response).await;
        assert_eq!(response_body.data.amount, request_body.payment.amount);
        assert_eq!(
            response_body.data.card_number,
            request_body.payment.card_number
        );
        assert_eq!(response_body.data.status, expected_status);
        assert!((expected_status_code == StatusCode::CREATED) ^ response_body.data.id.is_nil())
    }

    #[tokio::test]
    async fn should_approve_valid_payment() {
        let router = BankWeb::new_test().await.into_router();
        let payment_amount = 12_05;
        let payment_card_number: String = Card::new_test().into();
        do_payment(
            &router,
            payment_amount,
            payment_card_number.clone(),
            StatusCode::CREATED,
            Status::Approved,
        )
        .await;
    }

    #[tokio::test]
    async fn should_decline_payment_and_return_402_with_insufficient_funds() {
        let router = BankWeb::new_test_with_response("insufficient_funds")
            .await
            .into_router();

        do_payment(
            &router,
            12_05,
            Card::new_test().into(),
            StatusCode::PAYMENT_REQUIRED,
            Status::Declined,
        )
        .await;
    }

    #[tokio::test]
    async fn should_decline_payment_and_return_403_for_invalid_account_number() {
        let router = BankWeb::new_test_with_response("invalid_account_number")
            .await
            .into_router();

        do_payment(
            &router,
            12_05,
            Card::new_test().into(),
            StatusCode::FORBIDDEN,
            Status::Declined,
        )
        .await;
    }

    #[tokio::test]
    async fn should_fail_payment_and_return_503_for_service_unavailable() {
        let router = BankWeb::new_test_with_response("service_unavailable")
            .await
            .into_router();

        do_payment(
            &router,
            12_05,
            Card::new_test().into(),
            StatusCode::SERVICE_UNAVAILABLE,
            Status::Failed,
        )
        .await;
    }

    #[tokio::test]
    async fn should_fail_payment_and_return_500_for_internal_error() {
        let router = BankWeb::new_test_with_response("internal_error")
            .await
            .into_router();

        do_payment(
            &router,
            12_05,
            Card::new_test().into(),
            StatusCode::INTERNAL_SERVER_ERROR,
            Status::Failed,
        )
        .await;
    }

    #[tokio::test]
    async fn should_return_204_for_zero_amount() {
        let router = BankWeb::new_test().await.into_router();

        do_payment(
            &router,
            0,
            Card::new_test().into(),
            StatusCode::NO_CONTENT,
            Status::Declined,
        )
        .await;
    }

    #[tokio::test]
    async fn should_return_400_for_negative_amount() {
        let router = BankWeb::new_test().await.into_router();

        do_payment(
            &router,
            -1_00,
            Card::new_test().into(),
            StatusCode::BAD_REQUEST,
            Status::Declined,
        )
        .await;
    }

    #[tokio::test]
    async fn should_return_422_for_invalid_card_format() {
        let router = BankWeb::new_test().await.into_router();

        let mut invalid_card_number: String = Card::new_test().into();
        // TODO: parameterize to test with other invalid values?
        invalid_card_number.truncate(invalid_card_number.len() - 1);
        do_payment(
            &router,
            1_23,
            invalid_card_number,
            StatusCode::UNPROCESSABLE_ENTITY,
            Status::Declined,
        )
        .await;
    }

    #[tokio::test]
    async fn should_return_422_for_existing_card_number() {
        let router = BankWeb::new_test().await.into_router();
        let payment_card_number: String = Card::new_test().into();

        do_payment(
            &router,
            1_23,
            payment_card_number.clone(),
            StatusCode::CREATED,
            Status::Approved,
        )
        .await;
        do_payment(
            &router,
            1_23,
            payment_card_number,
            StatusCode::UNPROCESSABLE_ENTITY,
            Status::Declined,
        )
        .await;
    }
}
