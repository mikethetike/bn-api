use chrono::prelude::*;
use diesel;
use diesel::prelude::*;
use models::RefundItem;
use schema::*;
use utils::errors::*;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Identifiable, Queryable, Serialize, Deserialize)]
pub struct Refund {
    pub id: Uuid,
    pub order_id: Uuid,
    pub user_id: Uuid,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

impl Refund {
    pub fn create(order_id: Uuid, user_id: Uuid) -> NewRefund {
        NewRefund { order_id, user_id }
    }

    pub fn find(id: Uuid, conn: &PgConnection) -> Result<Refund, DatabaseError> {
        refunds::table
            .filter(refunds::id.eq(id))
            .first(conn)
            .to_db_error(ErrorCode::QueryError, "Could not retrieve refund data")
    }

    pub fn items(&self, conn: &PgConnection) -> Result<Vec<RefundItem>, DatabaseError> {
        refund_items::table
            .filter(refund_items::refund_id.eq(self.id))
            .load(conn)
            .to_db_error(ErrorCode::QueryError, "Could not load refund items")
    }
}

#[derive(Insertable, Clone)]
#[table_name = "refunds"]
pub struct NewRefund {
    pub order_id: Uuid,
    pub user_id: Uuid,
}

impl NewRefund {
    pub fn commit(self, conn: &PgConnection) -> Result<Refund, DatabaseError> {
        diesel::insert_into(refunds::table)
            .values(&self)
            .get_result(conn)
            .to_db_error(ErrorCode::InsertError, "Could not insert refund record")
    }
}