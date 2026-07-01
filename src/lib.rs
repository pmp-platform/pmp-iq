//! pmp-iq library crate.
//!
//! Exposes the application's modules so both the binary entrypoint and the
//! integration test suite can build and exercise the app.

pub mod accounts;
pub mod agent_tasks;
pub mod ai;
pub mod analysis_config;
pub mod app;
pub mod appsettings;
pub mod audit;
pub mod auth;
pub mod c4;
pub mod campaigns;
pub mod codebase_map;
pub mod config;
pub mod cost;
pub mod crypto;
pub mod dashboard;
pub mod db;
pub mod dora;
pub mod embeddings;
pub mod error;
pub mod files;
pub mod fs;
pub mod gamification;
pub mod git;
pub mod hints;
pub mod httpclient;
pub mod incremental;
pub mod jobs;
pub mod llm_request;
pub mod locks;
pub mod metrics;
pub mod nl_query;
pub mod platform;
pub mod pr_watcher;
pub mod process;
pub mod rbac;
pub mod remediation;
pub mod repositories;
pub mod review;
pub mod routes;
pub mod scorecards;
pub mod store;
pub mod strings;
pub mod techradar;
pub mod telemetry;
pub mod web;
pub mod workspace;
