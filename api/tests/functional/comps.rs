use actix_web::error::ResponseError;
use actix_web::{http::StatusCode, FromRequest, HttpResponse, Json, Path};
use bigneon_api::controllers::comps::{self, NewCompRequest};
use bigneon_api::controllers::holds::UpdateHoldRequest;
use bigneon_api::models::PathParameters;
use bigneon_db::models::*;
use functional::base;
use support;
use support::database::TestDatabase;
use support::test_request::TestRequest;

#[cfg(test)]
mod index_tests {
    use super::*;
    #[test]
    fn index_org_member() {
        base::comps::index(Roles::OrgMember, true);
    }
    #[test]
    fn index_admin() {
        base::comps::index(Roles::Admin, true);
    }
    #[test]
    fn index_user() {
        base::comps::index(Roles::User, false);
    }
    #[test]
    fn index_org_owner() {
        base::comps::index(Roles::OrgOwner, true);
    }
}

#[cfg(test)]
mod show_tests {
    use super::*;
    #[test]
    fn show_org_member() {
        base::comps::show(Roles::OrgMember, true);
    }
    #[test]
    fn show_admin() {
        base::comps::show(Roles::Admin, true);
    }
    #[test]
    fn show_user() {
        base::comps::show(Roles::User, false);
    }
    #[test]
    fn show_org_owner() {
        base::comps::show(Roles::OrgOwner, true);
    }
}

#[cfg(test)]
mod create_tests {
    use super::*;
    #[test]
    fn create_org_member() {
        base::comps::create(Roles::OrgMember, true);
    }
    #[test]
    fn create_admin() {
        base::comps::create(Roles::Admin, true);
    }
    #[test]
    fn create_user() {
        base::comps::create(Roles::User, false);
    }
    #[test]
    fn create_org_owner() {
        base::comps::create(Roles::OrgOwner, true);
    }
}

#[cfg(test)]
mod update_tests {
    use super::*;
    #[test]
    fn update_org_member() {
        base::comps::update(Roles::OrgMember, true);
    }
    #[test]
    fn update_admin() {
        base::comps::update(Roles::Admin, true);
    }
    #[test]
    fn update_user() {
        base::comps::update(Roles::User, false);
    }
    #[test]
    fn update_org_owner() {
        base::comps::update(Roles::OrgOwner, true);
    }
}

#[cfg(test)]
mod destroy_tests {
    use super::*;
    #[test]
    fn destroy_org_member() {
        base::comps::destroy(Roles::OrgMember, true);
    }
    #[test]
    fn destroy_admin() {
        base::comps::destroy(Roles::Admin, true);
    }
    #[test]
    fn destroy_user() {
        base::comps::destroy(Roles::User, false);
    }
    #[test]
    fn destroy_org_owner() {
        base::comps::destroy(Roles::OrgOwner, true);
    }
}

#[test]
fn create_with_validation_errors() {
    let database = TestDatabase::new();
    let connection = database.connection.clone();
    let user = database.create_user().finish();
    let hold = database
        .create_hold()
        .with_hold_type(HoldTypes::Comp)
        .finish();
    let event = Event::find(hold.event_id, &connection).unwrap();
    let organization = event.organization(&connection).unwrap();
    let auth_user =
        support::create_auth_user_from_user(&user, Roles::OrgOwner, Some(&organization), &database);

    let name = "Comp Example".to_string();
    let email = Some("invalid".to_string());
    let quantity = 10;

    let json = Json(NewCompRequest {
        name: name.clone(),
        email: email.clone(),
        phone: None,
        quantity,
        redemption_code: "OHHHYEAAAH".to_string(),
        end_at: None,
        max_per_order: None,
    });

    let test_request = TestRequest::create();
    let mut path = Path::<PathParameters>::extract(&test_request.request).unwrap();
    path.id = hold.id;

    let response = comps::create((database.connection.into(), json, path, auth_user));
    let err = response.err().unwrap();

    let response: HttpResponse = err.error_response();
    let validation_response = support::validation_response_from_response(&response).unwrap();
    let email = validation_response.fields.get("email").unwrap();
    assert_eq!(email[0].code, "email");
    assert_eq!(
        &email[0].message.clone().unwrap().into_owned(),
        "Email is invalid"
    );
}

#[test]
fn update_with_validation_errors() {
    let database = TestDatabase::new();
    let connection = database.connection.clone();
    let user = database.create_user().finish();
    let comp = database.create_comp().finish();
    let event = Event::find(comp.event_id, &connection).unwrap();
    let organization = event.organization(&connection).unwrap();
    let auth_user =
        support::create_auth_user_from_user(&user, Roles::OrgOwner, Some(&organization), &database);

    let email = "invalid".to_string();
    let test_request = TestRequest::create_with_uri_custom_params("/", vec!["id"]);
    let mut path = Path::<PathParameters>::extract(&test_request.request).unwrap();
    path.id = comp.id;

    let json = Json(UpdateHoldRequest {
        email: Some(Some(email)),
        ..Default::default()
    });

    let response: HttpResponse =
        comps::update((database.connection.into(), json, path, auth_user)).into();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(response.error().is_some());

    let validation_response = support::validation_response_from_response(&response).unwrap();
    let email = validation_response.fields.get("email").unwrap();
    assert_eq!(email[0].code, "email");
    assert_eq!(
        &email[0].message.clone().unwrap().into_owned(),
        "Email is invalid"
    );
}
