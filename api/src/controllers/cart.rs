use actix_web::HttpResponse;
use actix_web::Json;
use actix_web::Path;
use actix_web::State;
use auth::user::User;
use bigneon_db::models::{DisplayOrderItem, Order, OrderStatus, OrderTypes, PaymentStatus, Scopes};
use bigneon_db::utils::errors::Optional;
use db::Connection;
use errors::BigNeonError;
use helpers::application;
use models::PathParameters;
use server::AppState;
use stripe::StripeClient;
use uuid::Uuid;

#[derive(Serialize)]
pub struct CartResponse {
    pub cart_id: Uuid,
}

#[derive(Deserialize)]
pub struct AddToCartRequest {
    pub ticket_type_id: Uuid,
    pub quantity: i64,
}

pub fn add(
    (connection, json, user): (Connection, Json<AddToCartRequest>, User),
) -> Result<HttpResponse, BigNeonError> {
    let connection = connection.get();
    // Find the current cart of the user, if it exists.
    let current_cart = Order::find_cart_for_user(user.id(), connection).optional()?;
    let cart: Order;

    // Create it if there isn't one
    if current_cart.is_none() {
        cart = Order::create(user.id(), OrderTypes::Cart).commit(connection)?;
    } else {
        cart = current_cart.unwrap();
    }

    // Add the item
    cart.add_tickets(json.ticket_type_id, json.quantity, connection)?;

    Ok(HttpResponse::Created().json(&CartResponse { cart_id: cart.id }))
}

#[derive(Deserialize)]
pub struct RemoveCartRequest {
    pub cart_item_id: Uuid,
    pub quantity: Option<i64>,
}

pub fn remove(
    (connection, json, user): (Connection, Json<RemoveCartRequest>, User),
) -> Result<HttpResponse, BigNeonError> {
    let connection = connection.get();
    // Find the current cart of the user, if it exists.
    let current_cart = Order::find_cart_for_user(user.id(), connection).optional()?;

    match current_cart {
        Some(cart) => match cart.find_item(json.cart_item_id, connection).optional()? {
            Some(mut order_item) => {
                cart.remove_tickets(order_item, json.quantity, connection)?;
                Ok(HttpResponse::Ok().json(&CartResponse { cart_id: cart.id }))
            }
            None => application::unprocessable("Cart does not contain order item"),
        },
        None => application::unprocessable("No cart exists for user"),
    }
}

pub fn show((connection, user): (Connection, User)) -> Result<HttpResponse, BigNeonError> {
    let connection = connection.get();
    let order = Order::find_cart_for_user(user.id(), connection).optional()?;
    if order.is_none() {
        return Ok(HttpResponse::Ok().json(json!({})));
    }

    let order = order.unwrap();

    #[derive(Serialize)]
    struct DisplayCart {
        id: Uuid,
        items: Vec<DisplayOrderItem>,
        total_in_cents: i64,
    }

    let items = order.items_for_display(connection)?;
    let r = DisplayCart {
        id: order.id,
        items,
        total_in_cents: order.calculate_total(connection)?,
    };

    Ok(HttpResponse::Ok().json(r))
}

#[derive(Deserialize)]
pub struct CheckoutCartRequest {
    pub amount: i64,
    pub method: PaymentRequest,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
pub enum PaymentRequest {
    External { reference: String },
    Stripe { token: String },
}

pub fn checkout(
    (connection, json, path, user, state): (
        Connection,
        Json<CheckoutCartRequest>,
        Path<PathParameters>,
        User,
        State<AppState>,
    ),
) -> Result<HttpResponse, BigNeonError> {
    let req = json.into_inner();
    match &req.method {
        PaymentRequest::External { reference } => {
            checkout_external(connection, path.id, reference, &req, user)
        }
        PaymentRequest::Stripe { token } => checkout_stripe(
            connection,
            path.id,
            &token,
            &req,
            user,
            &state.config.primary_currency,
            &state.config.stripe_secret_key,
        ),
    }
}

// TODO: This should actually probably move to an `orders` controller, since the
// user will not be calling this.
fn checkout_external(
    conn: Connection,
    order_id: Uuid,
    reference: &String,
    checkout_request: &CheckoutCartRequest,
    user: User,
) -> Result<HttpResponse, BigNeonError> {
    let connection = conn.get();
    if !user.has_scope(Scopes::OrderMakeExternalPayment, None, connection)? {
        return application::unauthorized();
    }

    let mut order = Order::find(order_id, connection)?;

    if order.status() != OrderStatus::Draft {
        return application::unprocessable(
            "Could not complete this cart because it is not in the correct status",
        );
    }

    let payment = order.add_external_payment(
        reference.to_string(),
        user.id(),
        checkout_request.amount,
        connection,
    )?;

    Ok(HttpResponse::Ok().json(json!({"payment_id": payment.id})))
}

fn checkout_stripe(
    conn: Connection,
    order_id: Uuid,
    token: &str,
    req: &CheckoutCartRequest,
    user: User,
    currency: &str,
    stripe_api_key: &str,
) -> Result<HttpResponse, BigNeonError> {
    let connection = conn.get();

    let mut order = Order::find(order_id, connection)?;

    if order.user_id != user.id() {
        return application::forbidden("This cart does not belong to you");
    }

    if order.status() != OrderStatus::Draft {
        return application::unprocessable(
            "Could not complete this cart because it is not in the correct status",
        );
    }

    let client = StripeClient::new(stripe_api_key.to_string());
    let auth_result = client.auth(
        token,
        req.amount,
        currency,
        "Tickets from Bigneon",
        vec![("order_id".to_string(), order_id.to_string())],
    )?;

    let payment = match order.add_credit_card_payment(
        user.id(),
        req.amount,
        "Stripe".to_string(),
        auth_result.id.clone(),
        PaymentStatus::Authorized,
        auth_result.to_json(),
        connection,
    ) {
        Ok(p) => p,
        Err(e) => {
            client.refund(&auth_result.id)?;
            return Err(e.into());
        }
    };

    conn.commit_transaction()?;
    conn.begin_transaction()?;

    let charge_result = client.complete(&auth_result.id)?;
    match payment.mark_complete(charge_result.to_json(), connection) {
        Ok(_) => Ok(HttpResponse::Ok().json(json!({"payment_id": payment.id}))),
        Err(e) => {
            client.refund(&auth_result.id)?;
            return Err(e.into());
        }
    }
}
