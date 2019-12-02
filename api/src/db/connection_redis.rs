use actix_web::{Body::Binary, FromRequest, HttpRequest, HttpResponse, Result};
use chrono::Utc;
use errors::BigNeonError;
use r2d2_redis::redis::{Commands, RedisResult};
use r2d2_redis::RedisConnectionManager;
use server::AppState;
use std::str;
use std::sync::Arc;

pub struct ConnectionRedis {
    pub inner: Arc<r2d2_redis::r2d2::Pool<RedisConnectionManager>>,
}

impl Clone for ConnectionRedis {
    fn clone(&self) -> Self {
        ConnectionRedis {
            inner: self.inner.clone(),
        }
    }
}

pub trait RedisCommands {
    fn get_value(&mut self, key: &str) -> RedisResult<String>;
    fn set_value(&mut self, key: &str, value: &str) -> RedisResult<String>;
    fn get_value_int(&mut self, key: &str) -> RedisResult<i64>;
    fn set_value_int(&mut self, key: &str, value: i64) -> RedisResult<i64>;
    fn delete(&mut self, key: &str);
    fn is_key_outdated(&mut self, start_time: i64, seconds: i64) -> bool;

    fn get_cache_value(&mut self, key: &str, time_lapse: i64) -> Option<String>;
    fn set_cache_value(&mut self, key: &str, cached_value: &str);
}

impl RedisCommands for r2d2_redis::r2d2::PooledConnection<RedisConnectionManager> {
    fn get_value(&mut self, key: &str) -> RedisResult<String> {
        self.get(key)
    }
    fn set_value(&mut self, key: &str, value: &str) -> RedisResult<String> {
        self.set(key, value)
    }
    fn get_value_int(&mut self, key: &str) -> RedisResult<i64> {
        self.get(key)
    }
    fn set_value_int(&mut self, key: &str, value: i64) -> RedisResult<i64> {
        self.set(key, value)
    }
    fn delete(&mut self, key: &str) {
        let _: () = self.del(key.to_string()).unwrap_or_default();
    }

    // start_time: this is measured in Unix time, the time in milliseconds from 1970-01-01
    // compares the difference in current time to giving
    fn is_key_outdated(&mut self, start_time: i64, seconds: i64) -> bool {
        Utc::now().timestamp_millis() - start_time > seconds
    }

    // time_lapse: this is measured in milli seconds. Only return the redis value if it happened in this period
    fn get_cache_value(&mut self, key: &str, time_lapse: i64) -> Option<String> {
        if let Some(set_time) = self.get_value_int(key).ok() {
            // get the time when query was set
            if !self.is_key_outdated(set_time, time_lapse) {
                // if not outdated return the value for the key
                // else return None
                return self.get_value(key).ok();
            }
        }
        None
    }

    fn set_cache_value(&mut self, key: &str, value: &str) {
        // set the current time and the new value for the key
        let time_now = Utc::now().timestamp_millis();
        self.set_value_int(key, time_now).ok();
        self.set_value(key, value).ok();
        ()
    }
}

impl ConnectionRedis {
    pub fn conn(self) -> Result<r2d2_redis::r2d2::PooledConnection<RedisConnectionManager>, r2d2::Error> {
        self.inner.get()
    }

    pub fn unwrap_body_to_string(response: &HttpResponse) -> Result<&str, &'static str> {
        match response.body() {
            Binary(binary) => Ok(str::from_utf8(binary.as_ref()).unwrap()),
            _ => Err("Unexpected response body"),
        }
    }
}

impl FromRequest<AppState> for ConnectionRedis {
    type Config = ();
    type Result = Result<ConnectionRedis, BigNeonError>;

    fn from_request(request: &HttpRequest<AppState>, _config: &Self::Config) -> Self::Result {
        if let Some(connection) = request.extensions().get::<ConnectionRedis>() {
            return Ok(connection.clone());
        }

        let connection = request.state().database.get_redis_connection();
        let connection = ConnectionRedis {
            inner: Arc::new(connection),
        };

        request.extensions_mut().insert(connection.clone());
        Ok(connection.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use r2d2_redis::RedisConnectionManager;
    use errors::BigNeonError;

    fn create_redis_connection_pool(
        database_url: &str,
    ) -> Result<r2d2_redis::r2d2::Pool<RedisConnectionManager>, BigNeonError> {
        let manager = RedisConnectionManager::new(database_url)?;
        let pool = r2d2_redis::r2d2::Pool::builder().build(manager)?;
        Ok(pool)
    }
    #[test]
    fn test_caching() {
        let conn = create_redis_connection_pool("redis://127.0.0.1/").unwrap();
        assert_eq!(2 + 2, 4);
    }
}
