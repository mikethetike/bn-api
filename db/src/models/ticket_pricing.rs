use chrono::NaiveDateTime;
use dev::times;
use diesel;
use diesel::dsl::{self, exists, select, sql};
use diesel::prelude::*;
use diesel::sql_types::{Bool, Timestamp, Uuid as dUuid};
use models::*;
use schema::{order_items, orders, ticket_pricing};
use std::borrow::Cow;
use utils::errors::*;
use uuid::Uuid;
use validator::*;
use validators::{self, *};

sql_function!(fn ticket_pricing_no_overlapping_periods(id: dUuid, ticket_type_id: dUuid, start_date: Timestamp, end_date: Timestamp, is_box_office_only: Bool, is_default_status: Bool) -> Bool);

#[derive(Clone, Identifiable, Associations, Queryable, PartialEq, Debug, Serialize)]
#[belongs_to(TicketType)]
#[table_name = "ticket_pricing"]
pub struct TicketPricing {
    pub id: Uuid,
    pub ticket_type_id: Uuid,
    pub name: String,
    pub status: TicketPricingStatus,
    pub price_in_cents: i64,
    pub start_date: NaiveDateTime,
    pub end_date: NaiveDateTime,
    pub is_box_office_only: bool,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
    pub previous_ticket_pricing_id: Option<Uuid>,
}

#[derive(AsChangeset, Clone, Default, Deserialize, Serialize)]
#[table_name = "ticket_pricing"]
pub struct TicketPricingEditableAttributes {
    pub name: Option<String>,
    pub price_in_cents: Option<i64>,
    pub start_date: Option<NaiveDateTime>,
    pub end_date: Option<NaiveDateTime>,
    pub is_box_office_only: Option<bool>,
}

impl TicketPricing {
    pub fn associated_with_active_orders(&self, conn: &PgConnection) -> Result<bool, DatabaseError> {
        select(exists(
            order_items::table
                .inner_join(orders::table.on(orders::id.eq(order_items::order_id)))
                .filter(orders::status.eq(OrderStatus::Paid).or(orders::expires_at.ge(dsl::now)))
                .filter(
                    sql("
                        order_items.ticket_pricing_id in
                        (WITH RECURSIVE ticket_pricing_r(id) AS (
                            SELECT tp.*
                            FROM ticket_pricing AS tp
                            WHERE tp.id =
                    ")
                    .bind::<dUuid, _>(self.id)
                    .sql(
                        "
                        UNION ALL
                        SELECT tp.*
                        FROM ticket_pricing_r AS p, ticket_pricing AS tp
                        WHERE p.previous_ticket_pricing_id = tp.id
                        )
                        SELECT id FROM ticket_pricing_r)
                    ",
                    ),
                ),
        ))
        .get_result(conn)
        .to_db_error(
            ErrorCode::QueryError,
            "Could not confirm if ticket pricing has associated orders",
        )
    }

    pub fn create(
        ticket_type_id: Uuid,
        name: String,
        start_date: NaiveDateTime,
        end_date: NaiveDateTime,
        price_in_cents: i64,
        is_box_office_only: bool,
        status: Option<TicketPricingStatus>,
        previous_ticket_pricing_id: Option<Uuid>,
    ) -> NewTicketPricing {
        NewTicketPricing {
            ticket_type_id,
            name,
            status: status.unwrap_or(TicketPricingStatus::Published),
            start_date,
            end_date,
            price_in_cents,
            is_box_office_only,
            previous_ticket_pricing_id,
        }
    }

    pub fn validate_record(&self, attributes: &TicketPricingEditableAttributes) -> Result<(), DatabaseError> {
        let validation_errors = validators::append_validation_error(
            Ok(()),
            "ticket_pricing.start_date",
            validators::start_date_valid(
                attributes.start_date.unwrap_or(self.start_date),
                attributes.end_date.unwrap_or(self.end_date),
            ),
        );
        Ok(validation_errors?)
    }

    pub fn has_changes(&self, attributes: &TicketPricingEditableAttributes) -> bool {
        !((attributes.name.is_none() || Some(self.name.clone()) == attributes.name)
            && (attributes.price_in_cents.is_none() || Some(self.price_in_cents) == attributes.price_in_cents)
            && (attributes.start_date.is_none() || Some(self.start_date) == attributes.start_date)
            && (attributes.end_date.is_none() || Some(self.end_date) == attributes.end_date)
            && (attributes.is_box_office_only.is_none()
                || Some(self.is_box_office_only) == attributes.is_box_office_only))
    }

    pub fn update(
        &self,
        attributes: TicketPricingEditableAttributes,
        current_user_id: Option<Uuid>,
        conn: &PgConnection,
    ) -> Result<TicketPricing, DatabaseError> {
        if self.has_changes(&attributes) {
            self.validate_record(&attributes)?;
            if self.affected_order_count(conn)? == 0
                || attributes.price_in_cents.is_none()
                || attributes.price_in_cents == Some(self.price_in_cents)
            {
                // No orders affected or price does not change, update existing record
                let result = diesel::update(self)
                    .set((&attributes, ticket_pricing::updated_at.eq(dsl::now)))
                    .get_result(conn)
                    .to_db_error(ErrorCode::UpdateError, "Could not update ticket_pricing");

                DomainEvent::create(
                    DomainEventTypes::TicketPricingUpdated,
                    format!("Ticketing pricing '{}' updated", self.name),
                    Tables::TicketPricing,
                    Some(self.id),
                    current_user_id,
                    Some(json!(&attributes)),
                )
                .commit(conn)?;
                result
            } else {
                // Orders affected, create new ticket pricing and delete old
                let new_ticket_pricing = TicketPricing::create(
                    self.ticket_type_id,
                    attributes.name.unwrap_or(self.name.clone()),
                    attributes.start_date.unwrap_or(self.start_date),
                    attributes.end_date.unwrap_or(self.end_date),
                    attributes.price_in_cents.unwrap(),
                    attributes.is_box_office_only.unwrap_or(self.is_box_office_only),
                    Some(self.status),
                    Some(self.id),
                );
                self.destroy(current_user_id, conn)?;
                new_ticket_pricing.commit(current_user_id, conn)
            }
        } else {
            Ok(self.clone())
        }
    }

    pub fn ticket_pricing_does_not_overlap_ticket_type_start_date(
        ticket_type: &TicketType,
        start_date: NaiveDateTime,
    ) -> Result<Result<(), ValidationError>, DatabaseError> {
        if ticket_type.start_date.unwrap_or(start_date) > start_date {
            let mut validation_error = create_validation_error(
                "ticket_pricing_overlapping_ticket_type_start_date",
                "Ticket pricing dates overlap ticket type start date",
            );
            validation_error.add_param(Cow::from("ticket_type_id"), &ticket_type.id);
            validation_error.add_param(Cow::from("start_date"), &start_date);

            return Ok(Err(validation_error));
        }
        Ok(Ok(()))
    }

    pub fn ticket_pricing_does_not_overlap_ticket_type_end_date(
        ticket_type: &TicketType,
        end_date: NaiveDateTime,
        conn: &PgConnection,
    ) -> Result<Result<(), ValidationError>, DatabaseError> {
        if ticket_type.end_date(conn)? < end_date {
            let mut validation_error = create_validation_error(
                "ticket_pricing_overlapping_ticket_type_end_date",
                "Ticket pricing dates overlap ticket type end date",
            );
            validation_error.add_param(Cow::from("ticket_type_id"), &ticket_type.id);
            validation_error.add_param(Cow::from("end_date"), &end_date);

            return Ok(Err(validation_error));
        }
        Ok(Ok(()))
    }

    pub fn ticket_pricing_no_overlapping_periods(
        id: Uuid,
        ticket_type_id: Uuid,
        start_date: NaiveDateTime,
        end_date: NaiveDateTime,
        is_box_office_only: bool,
        status: TicketPricingStatus,
        conn: &PgConnection,
    ) -> Result<Result<(), ValidationError>, DatabaseError> {
        let is_default = status == TicketPricingStatus::Default;
        let result = select(ticket_pricing_no_overlapping_periods(
            id,
            ticket_type_id,
            start_date,
            end_date,
            is_box_office_only,
            is_default,
        ))
        .get_result::<bool>(conn)
        .to_db_error(ErrorCode::UpdateError, "Could not confirm periods do not overlap")?;
        if !result {
            let mut validation_error = create_validation_error(
                "ticket_pricing_overlapping_periods",
                "Ticket pricing dates overlap another ticket pricing period",
            );
            validation_error.add_param(Cow::from("ticket_pricing_id"), &id);
            validation_error.add_param(Cow::from("ticket_type_id"), &ticket_type_id);
            validation_error.add_param(Cow::from("start_date"), &start_date);
            validation_error.add_param(Cow::from("end_date"), &end_date);

            return Ok(Err(validation_error));
        }
        Ok(Ok(()))
    }

    pub fn affected_order_count(&self, conn: &PgConnection) -> Result<i64, DatabaseError> {
        order_items::table
            .inner_join(orders::table.on(orders::id.eq(order_items::order_id)))
            .filter(order_items::ticket_pricing_id.eq(Some(self.id)))
            .select(dsl::count(order_items::id))
            .first(conn)
            .to_db_error(ErrorCode::QueryError, "Could not load order_items")
    }

    pub fn destroy(&self, current_user_id: Option<Uuid>, conn: &PgConnection) -> Result<(), DatabaseError> {
        //Check if there is any order items linked to this ticket pricing
        if self.affected_order_count(conn)? == 0 {
            //Ticket pricing is unused -> delete
            diesel::delete(self)
                .execute(conn)
                .to_db_error(ErrorCode::DeleteError, "Error removing ticket pricing")?;
            DomainEvent::create(
                DomainEventTypes::TicketPricingDeleted,
                format!("Ticket pricing '{}' deleted", &self.name),
                Tables::TicketPricing,
                Some(self.id),
                current_user_id,
                Some(json!(self)),
            )
            .commit(conn)?;
            Ok(())
        } else {
            //Ticket pricing is used -> mark status for deletion
            let result: TicketPricing = diesel::update(self)
                .set((
                    ticket_pricing::status.eq(TicketPricingStatus::Deleted),
                    ticket_pricing::updated_at.eq(dsl::now),
                ))
                .get_result(conn)
                .to_db_error(ErrorCode::UpdateError, "Could not update ticket_pricing status")?;
            DomainEvent::create(
                DomainEventTypes::TicketPricingDeleted,
                format!("Ticket pricing '{}' deleted", &result.name),
                Tables::TicketPricing,
                Some(self.id),
                current_user_id,
                Some(json!(result)),
            )
            .commit(conn)?;

            Ok(())
        }
    }

    pub fn start_sales(mut self, current_user_id: Option<Uuid>, conn: &PgConnection) -> Result<(), DatabaseError> {
        if self.start_date > times::now() {
            let old_start_date = self.start_date;
            self.start_date = times::now();
            diesel::update(&self)
                .set((
                    ticket_pricing::start_date.eq(self.start_date),
                    ticket_pricing::updated_at.eq(dsl::now),
                ))
                .execute(conn)
                .to_db_error(ErrorCode::UpdateError, "Could not update start date on Ticket Pricing")?;

            DomainEvent::create(
                DomainEventTypes::TicketPricingSalesStarted,
                format!("Sales have started on '{}'", self.name),
                Tables::TicketPricing,
                Some(self.id),
                current_user_id,
                Some(json!({"old_start_date": old_start_date, "new_start_date": self.start_date})),
            )
            .commit(conn)?;
        }
        Ok(())
    }

    pub fn get_default(ticket_type_id: Uuid, conn: &PgConnection) -> Result<TicketPricing, DatabaseError> {
        ticket_pricing::table
            .filter(ticket_pricing::ticket_type_id.eq(ticket_type_id))
            .filter(ticket_pricing::status.eq(TicketPricingStatus::Default))
            .first::<TicketPricing>(conn)
            .to_db_error(ErrorCode::QueryError, "Error loading default pricing")
    }

    pub fn find(id: Uuid, conn: &PgConnection) -> Result<TicketPricing, DatabaseError> {
        ticket_pricing::table
            .find(id)
            .first::<TicketPricing>(conn)
            .to_db_error(ErrorCode::QueryError, "Error loading ticket pricing")
    }

    pub fn get_current_ticket_pricing(
        ticket_type_id: Uuid,
        box_office_pricing: bool,
        get_default_pricing: bool,
        conn: &PgConnection,
    ) -> Result<TicketPricing, DatabaseError> {
        let mut ticket_pricing_status = TicketPricingStatus::Published;
        if get_default_pricing {
            ticket_pricing_status = TicketPricingStatus::Default
        }

        let mut query = ticket_pricing::table
            .filter(ticket_pricing::ticket_type_id.eq(ticket_type_id))
            .filter(ticket_pricing::status.eq(ticket_pricing_status))
            .filter(ticket_pricing::start_date.le(dsl::now))
            .filter(ticket_pricing::end_date.gt(dsl::now))
            .into_boxed();

        if box_office_pricing {
            // Use is_box_office_only pricing, fall back to regular pricing if not set
            query = query.order(ticket_pricing::is_box_office_only.desc()).limit(1);
        } else {
            query = query.filter(ticket_pricing::is_box_office_only.eq(false));
        }

        let mut price_points = query
            .load(conn)
            .to_db_error(ErrorCode::QueryError, "Could not load Ticket Pricing")?;

        if price_points.len() > 1 {
            return Err(DatabaseError::new(
                ErrorCode::MultipleResultsWhenOneExpected,
                Some("Expected a single ticket pricing period but multiple were found".to_string()),
            ));
        } else if price_points.len() == 0 {
            if get_default_pricing == false {
                return TicketPricing::get_current_ticket_pricing(ticket_type_id, box_office_pricing, true, &conn);
            } else {
                return Err(DatabaseError::new(
                    ErrorCode::NoResults,
                    Some("No ticket pricing found".to_string()),
                ));
            }
        }

        price_points.pop().ok_or(DatabaseError::new(
            ErrorCode::NoResults,
            Some("No ticket pricing found".to_string()),
        ))
    }
}

#[derive(Clone, Insertable)]
#[table_name = "ticket_pricing"]
pub struct NewTicketPricing {
    ticket_type_id: Uuid,
    name: String,
    status: TicketPricingStatus,
    price_in_cents: i64,
    is_box_office_only: bool,
    pub start_date: NaiveDateTime,
    pub end_date: NaiveDateTime,
    previous_ticket_pricing_id: Option<Uuid>,
}

impl NewTicketPricing {
    pub fn validate_record(&self) -> Result<(), DatabaseError> {
        let validation_errors = validators::append_validation_error(
            Ok(()),
            "ticket_pricing.start_date",
            validators::start_date_valid(self.start_date, self.end_date),
        );

        let validation_errors = validators::append_validation_error(
            validation_errors,
            "ticket_pricing.price_in_cents",
            validators::validate_greater_than_or_equal(
                self.price_in_cents,
                0,
                "number_must_be_positive",
                "Ticket price must be positive",
            ),
        );

        Ok(validation_errors?)
    }

    pub fn commit(self, current_user_id: Option<Uuid>, conn: &PgConnection) -> Result<TicketPricing, DatabaseError> {
        self.validate_record()?;
        let result: TicketPricing = diesel::insert_into(ticket_pricing::table)
            .values(self)
            .get_result(conn)
            .to_db_error(ErrorCode::InsertError, "Could not create ticket pricing")?;

        DomainEvent::create(
            DomainEventTypes::TicketPricingCreated,
            format!("Ticket pricing '{}' created", result.id),
            Tables::TicketPricing,
            Some(result.id),
            current_user_id,
            Some(json!(result)),
        )
        .commit(conn)?;
        Ok(result)
    }
}
