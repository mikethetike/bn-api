use actix_web::{HttpResponse, Path, Query};
use auth::user::User as AuthUser;
use bigneon_db::models::*;
use db::Connection;
use errors::*;
use extractors::*;
use models::PathParameters;

pub fn index(
    (connection, path_parameters, query_parameters): (Connection, Path<PathParameters>, Query<PagingParameters>),
) -> Result<HttpResponse, BigNeonError> {
    let stages = Stage::find_by_venue_id(path_parameters.id, connection.get())?;

    Ok(HttpResponse::Ok().json(&Payload::from_data(
        stages,
        query_parameters.page(),
        query_parameters.limit(),
        None,
    )))
}

pub fn show((connection, parameters): (Connection, Path<PathParameters>)) -> Result<HttpResponse, BigNeonError> {
    let connection = connection.get();
    let stage = Stage::find(parameters.id, connection)?;

    Ok(HttpResponse::Ok().json(&stage))
}

#[derive(Deserialize)]
pub struct CreateStage {
    pub name: String,
    pub description: Option<String>,
    pub capacity: Option<i64>,
}

pub fn create(
    (connection, parameters, create_stage, user): (Connection, Path<PathParameters>, Json<CreateStage>, AuthUser),
) -> Result<HttpResponse, BigNeonError> {
    let connection = connection.get();
    let venue = Venue::find(parameters.id, connection)?;

    if let Some(organization_id) = venue.organization_id {
        let organization = Organization::find(organization_id, connection)?;
        user.requires_scope_for_organization(Scopes::VenueWrite, &organization, connection)?;
    } else {
        user.requires_scope(Scopes::VenueWrite)?;
    }

    let new_stage = Stage::create(
        parameters.id,
        create_stage.name.clone(),
        create_stage.description.clone(),
        create_stage.capacity.clone(),
    );
    let stage = new_stage.commit(connection)?;

    Ok(HttpResponse::Created().json(&stage))
}

pub fn update(
    (connection, parameters, stage_parameters, user): (
        Connection,
        Path<PathParameters>,
        Json<StageEditableAttributes>,
        AuthUser,
    ),
) -> Result<HttpResponse, BigNeonError> {
    let connection = connection.get();
    let stage = Stage::find(parameters.id, connection)?;
    let venue = Venue::find(stage.venue_id, connection)?;
    if !venue.is_private || venue.organization_id.is_none() {
        user.requires_scope(Scopes::VenueWrite)?;
    } else {
        let organization = venue.organization(connection)?.unwrap();
        user.requires_scope_for_organization(Scopes::VenueWrite, &organization, connection)?;
    }

    let updated_stage = stage.update(stage_parameters.into_inner(), connection)?;
    Ok(HttpResponse::Ok().json(updated_stage))
}

pub fn delete(
    (connection, parameters, user): (Connection, Path<PathParameters>, AuthUser),
) -> Result<HttpResponse, BigNeonError> {
    let connection = connection.get();
    let stage = Stage::find(parameters.id, connection)?;
    let venue = Venue::find(stage.venue_id, connection)?;
    if !venue.is_private || venue.organization_id.is_none() {
        user.requires_scope(Scopes::VenueWrite)?;
    } else {
        let organization = venue.organization(connection)?.unwrap();
        user.requires_scope_for_organization(Scopes::VenueWrite, &organization, connection)?;
    }

    stage.destroy(connection)?;
    Ok(HttpResponse::Ok().json(json!({})))
}
