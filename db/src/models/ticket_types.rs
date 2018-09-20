use chrono::NaiveDateTime;
use diesel;
use diesel::expression::dsl::count;
use diesel::prelude::*;
use models::{Event, TicketInstance, TicketPricing, TicketTypeStatus};
use schema::{assets, ticket_instances, ticket_pricing, ticket_types};
use utils::errors::ConvertToDatabaseError;
use utils::errors::DatabaseError;
use utils::errors::ErrorCode;
use uuid::Uuid;

#[derive(Identifiable, Associations, Queryable, PartialEq, Debug)]
#[table_name = "ticket_types"]
#[belongs_to(Event)]
pub struct TicketType {
    pub id: Uuid,
    pub event_id: Uuid,
    pub name: String,
    status: String,
    pub start_date: NaiveDateTime,
    pub end_date: NaiveDateTime,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
}

impl TicketType {
    pub fn create(
        event_id: Uuid,
        name: String,
        start_date: NaiveDateTime,
        end_date: NaiveDateTime,
    ) -> NewTicketType {
        NewTicketType {
            event_id,
            name,
            status: TicketTypeStatus::Published.to_string(),
            start_date,
            end_date,
        }
    }

    pub fn find(id: Uuid, conn: &PgConnection) -> Result<TicketType, DatabaseError> {
        ticket_types::table
            .filter(ticket_types::id.eq(id))
            .get_result(conn)
            .to_db_error(ErrorCode::QueryError, "Could not find ticket type")
    }

    pub fn find_by_event_id(
        event_id: Uuid,
        conn: &PgConnection,
    ) -> Result<Vec<TicketType>, DatabaseError> {
        ticket_types::table
            .filter(ticket_types::event_id.eq(event_id))
            .order_by(ticket_types::name)
            .load(conn)
            .to_db_error(
                ErrorCode::QueryError,
                "Could not load ticket types for event",
            )
    }

    pub fn ticket_pricing(&self, conn: &PgConnection) -> Result<Vec<TicketPricing>, DatabaseError> {
        ticket_pricing::table
            .filter(ticket_pricing::ticket_type_id.eq(self.id))
            .order_by(ticket_pricing::name)
            .load(conn)
            .to_db_error(
                ErrorCode::QueryError,
                "Could not load ticket pricing for ticket type",
            )
    }

    pub fn ticket_capacity(&self, conn: &PgConnection) -> Result<u32, DatabaseError> {
        //Calculate capacity by counting the number of ticket instances for event
        let ticket_capacity: i64 = assets::table
            .filter(assets::ticket_type_id.eq(self.id))
            .inner_join(ticket_instances::table)
            .select(count(ticket_instances::id))
            .first(conn)
            .to_db_error(ErrorCode::QueryError, "Unable to load ticket instances")?;
        Ok(ticket_capacity as u32)
    }

    pub fn add_ticket_pricing(
        &self,
        name: String,
        start_date: NaiveDateTime,
        end_date: NaiveDateTime,
        price_in_cents: i64,
        conn: &PgConnection,
    ) -> Result<TicketPricing, DatabaseError> {
        TicketPricing::create(self.id, name, start_date, end_date, price_in_cents).commit(conn)
    }

    pub fn status(&self) -> TicketTypeStatus {
        TicketTypeStatus::parse(&self.status).unwrap()
    }
}

#[derive(Insertable)]
#[table_name = "ticket_types"]
pub struct NewTicketType {
    event_id: Uuid,
    name: String,
    status: String,
    start_date: NaiveDateTime,
    end_date: NaiveDateTime,
}

impl NewTicketType {
    pub fn commit(self, conn: &PgConnection) -> Result<TicketType, DatabaseError> {
        diesel::insert_into(ticket_types::table)
            .values(self)
            .get_result(conn)
            .to_db_error(ErrorCode::InsertError, "Could not create ticket type")
    }
}
