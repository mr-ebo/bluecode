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
        bank_web::tests::{deserialize_response_body, get, post},
    };

    #[tokio::test]
    async fn should_approve_valid_payment() {
        let router = BankWeb::new_test().await.into_router();

        let request_body = RequestBody {
            payment: RequestData {
                amount: 1205,
                card_number: Card::new_test().into(),
            },
        };

        let response = post(&router, "/api/payments", &request_body).await;
        assert_eq!(response.status(), 201);

        let response_body = deserialize_response_body::<ResponseBody>(response).await;
        assert_eq!(response_body.data.amount, request_body.payment.amount);

        let uri = format!("/api/payments/{}", response_body.data.id);
        let response = get(&router, uri).await;
        assert_eq!(response.status(), 200);

        let response_body = deserialize_response_body::<ResponseBody>(response).await;
        assert_eq!(response_body.data.amount, request_body.payment.amount);
        assert_eq!(response_body.data.status, Status::Approved);
    }

    #[tokio::test]
    async fn should_decline_payment_and_return_402_with_insufficient_funds() {
        let router = BankWeb::new_test_with_response("insufficient_funds")
            .await
            .into_router();

        let request_body = RequestBody {
            payment: RequestData {
                amount: 1205,
                card_number: Card::new_test().into(),
            },
        };

        let response = post(&router, "/api/payments", &request_body).await;
        assert_eq!(response.status(), 402);

        let response_body = deserialize_response_body::<ResponseBody>(response).await;
        assert_eq!(response_body.data.amount, request_body.payment.amount);
        assert_eq!(response_body.data.status, Status::Declined);
    }

    #[tokio::test]
    async fn should_decline_payment_and_return_403_for_invalid_account_number() {
        let router = BankWeb::new_test_with_response("invalid_account_number")
            .await
            .into_router();

        let request_body = RequestBody {
            payment: RequestData {
                amount: 1205,
                card_number: Card::new_test().into(),
            },
        };

        let response = post(&router, "/api/payments", &request_body).await;
        assert_eq!(response.status(), 403);

        let response_body = deserialize_response_body::<ResponseBody>(response).await;
        assert_eq!(response_body.data.amount, request_body.payment.amount);
        assert_eq!(response_body.data.status, Status::Declined);
    }

    #[tokio::test]
    async fn should_return_204_for_zero_amount() {
        let router = BankWeb::new_test().await.into_router();

        let request_body = RequestBody {
            payment: RequestData {
                amount: 0,
                card_number: Card::new_test().into(),
            },
        };

        let response = post(&router, "/api/payments", &request_body).await;
        assert_eq!(response.status(), 204);
    }

    #[tokio::test]
    async fn should_return_422_for_existing_card_number() {
        let router = BankWeb::new_test().await.into_router();

        let request_body = RequestBody {
            payment: RequestData {
                amount: 123,
                card_number: Card::new_test().into(),
            },
        };

        let response = post(&router, "/api/payments", &request_body).await;
        assert_eq!(response.status(), 201);

        let response = post(&router, "/api/payments", &request_body).await;
        assert_eq!(response.status(), 422);
    }
}
