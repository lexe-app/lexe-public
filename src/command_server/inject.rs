//! This module contains a collection of warp `Filter`s which inject items that
//! are required for subsequent handlers to function.

// /// Injects a
// fn with_db(db: Db) -> impl Filter<Extract = (Db,), Error = Infallible> +
// Clone {     warp::any().map(move || db.clone())
// }
