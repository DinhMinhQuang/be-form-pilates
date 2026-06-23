use axum::{Json, Router, extract::State, routing::get};
use serde::Serialize;
use uuid::Uuid;

use crate::{error::AppError, state::AppState};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/branches", get(branches))
        .route("/class-types", get(class_types))
        .route("/packages", get(packages))
}

#[derive(Serialize)]
struct BranchView {
    id: Uuid,
    code: String,
    name: String,
    address: String,
    timezone: String,
    status: String,
    class_type_ids: Vec<Uuid>,
}

async fn branches(State(state): State<AppState>) -> Result<Json<Vec<BranchView>>, AppError> {
    let rows: Vec<(Uuid, String, String, String, String, String, Vec<Uuid>)> = sqlx::query_as(
        r#"SELECT b.id, b.code, b.name, b.address, b.timezone, b.status,
                  COALESCE(array_agg(bct.class_type_id ORDER BY ct.name)
                    FILTER (WHERE bct.enabled), ARRAY[]::uuid[])
           FROM branch b
           LEFT JOIN branch_class_type bct ON bct.branch_id = b.id
           LEFT JOIN class_type ct ON ct.id = bct.class_type_id
           GROUP BY b.id
           ORDER BY b.code"#,
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| BranchView {
                id: r.0,
                code: r.1,
                name: r.2,
                address: r.3,
                timezone: r.4,
                status: r.5,
                class_type_ids: r.6,
            })
            .collect(),
    ))
}

#[derive(Serialize)]
struct ClassTypeView {
    id: Uuid,
    code: String,
    name: String,
    description: String,
    category: String,
    level: String,
    default_capacity: i32,
    status: String,
    branch_ids: Vec<Uuid>,
}

async fn class_types(State(state): State<AppState>) -> Result<Json<Vec<ClassTypeView>>, AppError> {
    let rows: Vec<(
        Uuid,
        String,
        String,
        String,
        String,
        String,
        i32,
        String,
        Vec<Uuid>,
    )> = sqlx::query_as(
        r#"SELECT ct.id, ct.code, ct.name, ct.description, ct.category, ct.level,
                      ct.default_capacity, ct.status,
                      COALESCE(array_agg(bct.branch_id ORDER BY b.code)
                        FILTER (WHERE bct.enabled), ARRAY[]::uuid[])
               FROM class_type ct
               LEFT JOIN branch_class_type bct ON bct.class_type_id = ct.id
               LEFT JOIN branch b ON b.id = bct.branch_id
               GROUP BY ct.id
               ORDER BY ct.name"#,
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| ClassTypeView {
                id: r.0,
                code: r.1,
                name: r.2,
                description: r.3,
                category: r.4,
                level: r.5,
                default_capacity: r.6,
                status: r.7,
                branch_ids: r.8,
            })
            .collect(),
    ))
}

#[derive(Serialize)]
struct PackageView {
    id: Uuid,
    code: String,
    name: String,
    sessions: i32,
    validity_months: i32,
    status: String,
    class_type_ids: Vec<Uuid>,
}

async fn packages(State(state): State<AppState>) -> Result<Json<Vec<PackageView>>, AppError> {
    let rows: Vec<(Uuid, String, String, i32, i32, String, Vec<Uuid>)> = sqlx::query_as(
        r#"SELECT cp.id, cp.code, cp.name, cp.sessions, cp.validity_months, cp.status,
                  COALESCE(array_agg(pct.class_type_id ORDER BY ct.name)
                    FILTER (WHERE pct.class_type_id IS NOT NULL), ARRAY[]::uuid[])
           FROM course_package cp
           LEFT JOIN package_class_type pct ON pct.package_id = cp.id
           LEFT JOIN class_type ct ON ct.id = pct.class_type_id
           GROUP BY cp.id
           ORDER BY cp.sessions"#,
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| PackageView {
                id: r.0,
                code: r.1,
                name: r.2,
                sessions: r.3,
                validity_months: r.4,
                status: r.5,
                class_type_ids: r.6,
            })
            .collect(),
    ))
}
