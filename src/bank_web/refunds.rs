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
    use crate::bank::accounts::DummyService;
    use crate::{
        bank::{payment_instruments::Card, payments::Status},
        bank_web::{
            payments,
            tests::{deserialize_response_body, post},
        },
    };
    use axum::Router;
    use std::future::Future;

    async fn setup_successful_payment(payment_amount: i32) -> (Router, payments::ResponseBody) {
        let router = BankWeb::new_test().await.into_router();

        let request_body = payments::RequestBody {
            payment: payments::RequestData {
                amount: payment_amount,
                card_number: Card::new_test().into(),
            },
        };

        let response = post(&router, "/api/payments", &request_body).await;
        assert_eq!(response.status(), StatusCode::CREATED);

        let response_body = deserialize_response_body::<payments::ResponseBody>(response).await;
        assert_eq!(response_body.data.status, Status::Approved);

        (router, response_body)
    }

    async fn setup_failed_payment(
        bank_web: impl Future<Output = BankWeb<DummyService>>,
        payment_amount: i32,
        expected_status_code: StatusCode,
        expected_status: Status,
    ) -> (Router, payments::ResponseBody) {
        let router = bank_web.await.into_router();

        let request_body = payments::RequestBody {
            payment: payments::RequestData {
                amount: payment_amount,
                card_number: Card::new_test().into(),
            },
        };

        let response = post(&router, "/api/payments", &request_body).await;
        assert_eq!(response.status(), expected_status_code);

        let response_body = deserialize_response_body::<payments::ResponseBody>(response).await;
        assert_eq!(response_body.data.status, expected_status);

        (router, response_body)
    }

    async fn do_refund(
        router: &Router,
        refund_amount: i32,
        payment_id: Uuid,
        expected_status_code: StatusCode,
    ) {
        let request_body = RequestBody {
            refund: RequestData {
                amount: refund_amount,
            },
        };

        let uri = format!("/api/payments/{payment_id}/refunds",);
        let response = post(router, uri, &request_body).await;
        assert_eq!(response.status(), expected_status_code);

        let response_body = deserialize_response_body::<ResponseBody>(response).await;
        assert_eq!(response_body.data.amount, refund_amount);
        assert!(expected_status_code.is_success() ^ response_body.data.id.is_nil());
    }

    #[tokio::test]
    async fn should_full_refund() {
        let amount = 10_00;
        let (router, payment_response_body) = setup_successful_payment(amount).await;
        let payment_id = payment_response_body.data.id;

        do_refund(&router, amount, payment_id, StatusCode::CREATED).await;
    }

    #[tokio::test]
    async fn should_partial_refunds_up_to_payment_amount() {
        let (router, payment_response_body) = setup_successful_payment(10_00).await;
        let payment_id = payment_response_body.data.id;

        do_refund(&router, 2_00, payment_id, StatusCode::CREATED).await;
        do_refund(&router, 5_00, payment_id, StatusCode::CREATED).await;
        do_refund(&router, 3_00, payment_id, StatusCode::CREATED).await;
    }

    #[tokio::test]
    async fn should_reject_refund_of_unknown_payment() {
        let router = BankWeb::new_test().await.into_router();
        let payment_id: Uuid = Uuid::new_v4();

        do_refund(&router, 2_00, payment_id, StatusCode::NOT_FOUND).await;
    }

    #[tokio::test]
    async fn should_reject_refund_of_declined_payment() {
        let payment_amount = 1205;
        let (router, payment_response_body) = setup_failed_payment(
            BankWeb::new_test_with_response("insufficient_funds"),
            payment_amount,
            StatusCode::PAYMENT_REQUIRED,
            Status::Declined,
        )
        .await;
        let payment_id = payment_response_body.data.id;

        do_refund(&router, 2_00, payment_id, StatusCode::NOT_FOUND).await;
    }

    #[tokio::test]
    async fn should_reject_full_refund_with_excessive_amount() {
        let (router, payment_response_body) = setup_successful_payment(10_00).await;
        let payment_id = payment_response_body.data.id;

        do_refund(&router, 11_00, payment_id, StatusCode::UNPROCESSABLE_ENTITY).await;
    }

    #[tokio::test]
    async fn should_reject_partial_refund_with_excessive_amount() {
        let (router, payment_response_body) = setup_successful_payment(10_00).await;
        let payment_id = payment_response_body.data.id;

        do_refund(&router, 2_00, payment_id, StatusCode::CREATED).await;
        do_refund(&router, 9_00, payment_id, StatusCode::UNPROCESSABLE_ENTITY).await;
    }
}
