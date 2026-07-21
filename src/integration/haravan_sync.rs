use reqwest::Client;
use serde::Deserialize;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Deserialize)]
struct CollectionsResponse {
    custom_collections: Vec<HaravanCollection>,
}

#[derive(Deserialize)]
struct HaravanCollection {
    id: i64,
    title: String,
    handle: String,
}

#[derive(Deserialize)]
struct ProductsResponse {
    products: Vec<HaravanProduct>,
}

#[derive(Deserialize)]
struct HaravanProduct {
    id: i64,
    title: String,
    variants: Vec<HaravanVariant>,
}

#[derive(Deserialize)]
struct HaravanVariant {
    id: i64,
    title: String,
    sku: Option<String>,
}

fn parse_sku(sku: &str) -> Option<(i32, i32)> {
    let sessions = regex_capture(sku, r"-(\d+)s")?;
    let days = regex_capture(sku, r"-(\d+)d")?;
    Some((sessions, days))
}

fn regex_capture(s: &str, pattern: &str) -> Option<i32> {
    let re = regex::Regex::new(pattern).ok()?;
    re.captures(s)?.get(1)?.as_str().parse().ok()
}

// SKU prefix → danh sách category của class_type cần mở cho branch
fn sku_to_categories(sku: &str) -> &'static [&'static str] {
    if sku.starts_with("reformer-duo") {
        &["duo"]
    } else if sku.starts_with("reformer-private") {
        &["private"]
    } else if sku.starts_with("mat-") {
        &["group_mat"]
    } else if sku.starts_with("reformer-") {
        &["group_reformer"]
    } else {
        &[]
    }
}

pub async fn sync_products(pool: &PgPool) {
    let Ok(api_url) = std::env::var("HARAVAN_API_URL") else {
        tracing::warn!("HARAVAN_API_URL not set, skipping product sync");
        return;
    };
    let Ok(api_token) = std::env::var("HARAVAN_API_TOKEN") else {
        tracing::warn!("HARAVAN_API_TOKEN not set, skipping product sync");
        return;
    };

    match fetch_and_upsert(&api_url, &api_token, pool).await {
        Ok(count) => tracing::info!(count, "Haravan product sync complete"),
        Err(e) => tracing::error!(error = %e, "Haravan product sync failed"),
    }
}

async fn fetch_and_upsert(api_url: &str, api_token: &str, pool: &PgPool) -> anyhow::Result<usize> {
    let client = Client::new();
    let base = api_url.trim_end_matches('/');

    let collections: CollectionsResponse = client
        .get(format!("{base}/custom_collections.json?limit=250"))
        .bearer_auth(api_token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let mut upserted = 0;
    for collection in &collections.custom_collections {
        let collection_id_str = collection.id.to_string();

        let branch_id: Uuid = sqlx::query_scalar(
            r#"INSERT INTO branch (code, name, haravan_collection_id)
               VALUES ($1, $2, $3)
               ON CONFLICT (haravan_collection_id) DO UPDATE SET name = EXCLUDED.name
               RETURNING id"#,
        )
        .bind(&collection.handle)
        .bind(&collection.title)
        .bind(&collection_id_str)
        .fetch_one(pool)
        .await?;

        let products: ProductsResponse = client
            .get(format!(
                "{base}/products.json?collection_id={}&limit=250",
                collection.id
            ))
            .bearer_auth(api_token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        // Gom các category cần mở cho branch này từ tất cả SKU trong collection
        let mut categories: Vec<&str> = Vec::new();

        for product in &products.products {
            for variant in &product.variants {
                let sku = match &variant.sku {
                    Some(s) if !s.is_empty() => s.as_str(),
                    _ => {
                        tracing::debug!(variant_id = variant.id, title = %variant.title, "no SKU, skipping");
                        continue;
                    }
                };

                let Some((sessions, validity_days)) = parse_sku(sku) else {
                    tracing::debug!(sku, "SKU format mismatch, skipping");
                    continue;
                };

                // Gom category cho branch
                for cat in sku_to_categories(sku) {
                    if !categories.contains(cat) {
                        categories.push(cat);
                    }
                }

                let name = format!("{} - {}", product.title, variant.title);

                // Upsert package + mapping
                let pkg_result = sqlx::query_scalar::<_, Uuid>(
                    r#"WITH pkg AS (
                           INSERT INTO course_package (code, name, sessions, validity_days, haravan_sku)
                           VALUES ($1, $2, $3, $4, $1)
                           ON CONFLICT (haravan_sku) DO UPDATE
                               SET name = EXCLUDED.name,
                                   sessions = EXCLUDED.sessions,
                                   validity_days = EXCLUDED.validity_days,
                                   status = 'active'
                           RETURNING id
                       )
                       INSERT INTO haravan_product_mapping (haravan_product_id, haravan_variant_id, package_id, branch_id)
                       SELECT $5, $6, id, $7 FROM pkg
                       ON CONFLICT (haravan_variant_id) DO UPDATE
                           SET haravan_product_id = EXCLUDED.haravan_product_id,
                               branch_id = EXCLUDED.branch_id,
                               active = true
                       RETURNING package_id"#,
                )
                .bind(sku)
                .bind(&name)
                .bind(sessions)
                .bind(validity_days)
                .bind(product.id.to_string())
                .bind(variant.id.to_string())
                .bind(branch_id)
                .fetch_optional(pool)
                .await;

                match pkg_result {
                    Ok(Some(package_id)) => {
                        // Upsert package_class_type dựa vào category của SKU
                        for cat in sku_to_categories(sku) {
                            let _ = sqlx::query(
                                r#"INSERT INTO package_class_type (package_id, class_type_id)
                                   SELECT $1, id FROM class_type WHERE category = $2
                                   ON CONFLICT DO NOTHING"#,
                            )
                            .bind(package_id)
                            .bind(cat)
                            .execute(pool)
                            .await;
                        }
                        upserted += 1;
                    }
                    Ok(None) => {}
                    Err(e) => tracing::error!(sku, error = %e, "failed to upsert variant"),
                }
            }
        }

        // Upsert branch_class_type cho tất cả category đã gom được
        for cat in &categories {
            let _ = sqlx::query(
                r#"INSERT INTO branch_class_type (branch_id, class_type_id)
                   SELECT $1, id FROM class_type WHERE category = $2
                   ON CONFLICT DO NOTHING"#,
            )
            .bind(branch_id)
            .bind(cat)
            .execute(pool)
            .await;
        }

        if !categories.is_empty() {
            tracing::info!(
                branch = %collection.handle,
                ?categories,
                "branch_class_type synced"
            );
        }
    }

    Ok(upserted)
}
