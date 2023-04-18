use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::BankWeb;
use crate::bank::{accounts::AccountService, refunds};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RequestData {
    amount: i32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RequestBody {
    refund: RequestData,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ResponseData {
    id: Uuid,
    amount: i32,
    payment_id: Uuid,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ResponseBody {
    data: ResponseData,
}

pub async fn post<T: AccountService>(
    State(bank_web): State<BankWeb<T>>,
    Path(payment_id): Path<Uuid>,
    Json(body): Json<RequestBody>,
) -> (StatusCode, Json<ResponseBody>) {
    let refund_id = refunds::insert(&bank_web.pool, payment_id, body.refund.amount)
        .await
        .unwrap();

    (
        StatusCode::CREATED,
        Json(ResponseBody {
            data: ResponseData {
                id: refund_id,
                amount: body.refund.amount,
                payment_id,
            },
        }),
    )
}

pub async fn get<T: AccountService>(
    State(bank_web): State<BankWeb<T>>,
    Path((payment_id, refund_id)): Path<(Uuid, Uuid)>,
) -> (StatusCode, Json<ResponseBody>) {
    let data = refunds::get(&bank_web.pool, refund_id).await.unwrap();

    (
        StatusCode::OK,
        Json(ResponseBody {
            data: ResponseData {
                id: data.id,
                amount: data.amount,
                payment_id,
            },
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        bank::{payment_instruments::Card, payments::Status},
        bank_web::{
            payments,
            tests::{deserialize_response_body, get, post},
        },
    };

    async fn setup() -> (axum::Router, payments::ResponseBody) {
        let router = BankWeb::new_test().await.into_router();

        let request_body = payments::RequestBody {
            payment: payments::RequestData {
                amount: 1205,
                card_number: Card::new_test().into(),
            },
        };

        let response = post(&router, "/api/payments", &request_body).await;
        assert_eq!(response.status(), 201);

        let response_body = deserialize_response_body::<payments::ResponseBody>(response).await;
        assert_eq!(response_body.data.status, Status::Approved);

        (router, response_body)
    }

    #[tokio::test]
    async fn should_refund_valid_amount() {
        let (router, payment_response_body) = setup().await;
        let payment_id = payment_response_body.data.id;

        let request_body = RequestBody {
            refund: RequestData { amount: 42 },
        };

        let uri = format!("/api/payments/{payment_id}/refunds",);
        let response = post(&router, uri, &request_body).await;
        assert_eq!(response.status(), 201);

        let response_body = deserialize_response_body::<ResponseBody>(response).await;
        assert_eq!(response_body.data.amount, request_body.refund.amount);
        let refund_id = response_body.data.id;

        let uri = format!("/api/payments/{payment_id}/refunds/{refund_id}");
        let response = get(&router, uri).await;
        assert_eq!(response.status(), 200);

        let response_body = deserialize_response_body::<ResponseBody>(response).await;
        assert_eq!(response_body.data.amount, request_body.refund.amount);
    }

    #[tokio::test]
    async fn should_reject_refund_of_invalid_amount() {
        let (router, payment_response_body) = setup().await;
        let payment_id = payment_response_body.data.id;

        let request_body = RequestBody {
            refund: RequestData {
                amount: payment_response_body.data.amount + 1,
            },
        };

        let uri = format!("/api/payments/{payment_id}/refunds",);
        let response = post(&router, uri, &request_body).await;
        assert_eq!(response.status(), 422);
    }
}
