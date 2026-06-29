//! PlatIQ library crate.
//!
//! Exposes the application's modules so both the binary entrypoint and the
//! integration test suite can build and exercise the app.

pub mod accounts;
pub mod agent_tasks;
pub mod ai;
pub mod analysis_config;
pub mod app;
pub mod appsettings;
pub mod auth;
pub mod config;
pub mod crypto;
pub mod db;
pub mod error;
pub mod files;
pub mod fs;
pub mod git;
pub mod hints;
pub mod httpclient;
pub mod jobs;
pub mod llm_request;
pub mod locks;
pub mod nl_query;
pub mod platform;
pub mod pr_watcher;
pub mod process;
pub mod repositories;
pub mod review;
pub mod routes;
pub mod store;
pub mod strings;
pub mod telemetry;
pub mod web;
pub mod workspace;
