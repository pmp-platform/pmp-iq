//! Repository accounts: model, persistence, provider strategies, selection and
//! the orchestrating service.

pub mod model;
pub mod providers;
pub mod repository;
pub mod selector;
pub mod service;

pub use model::{
    AccountInput, AuthType, ProviderType, RemoteRepo, RepositoryAccount, SelectionMode,
};
pub use providers::{
    PrCheck, PrComment, PrStatus, ProviderDeps, ProviderError, PullRequest, PullRequestSpec,
    RepoMember, RepositoryProvider, RepositoryProviderFactory,
};
pub use repository::{
    PgRepositoryAccountRepository, RepositoryAccountRepository, SqliteRepositoryAccountRepository,
};
pub use selector::{RepoSelector, SelectionError};
pub use service::{AccountForm, AccountService};
