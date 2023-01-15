use anyhow::Result;
use skim::SkimOptions;
use thiserror::Error;

pub trait CTX {
    fn auth(&mut self, profile: &str) -> Result<Context, CTXError>;
    fn list_contexts(&self) -> Result<Vec<Context>, CTXError>;
    fn get_active_context(&self) -> Result<Context, CTXError>;
    fn set_default_profile(
        &mut self,
        profile: &str,
    ) -> Result<Context, CTXError>;
    fn dump_credentials(&self) -> Result<(), CTXError>;
    fn use_context(&mut self, profile: &str) -> Result<Context, CTXError>;
    fn use_context_interactive(
        &mut self,
        skim_options: SkimOptions,
    ) -> Result<Context, CTXError>;
}

#[derive(Error, Debug)]
pub enum CTXError {
    #[error("Cannot read credentials")]
    CannotReadCredentials { source: Option<anyhow::Error> },
    #[error("Cannot write credentials")]
    CannotWriteCredentials { source: Option<anyhow::Error> },
    #[error("Credentials is broken")]
    CredentialsIsBroken { source: Option<anyhow::Error> },
    #[error("Invalid configurations")]
    InvalidConfigurations {
        message: String,
        source: Option<anyhow::Error>,
    },
    #[error("No active context found")]
    NoActiveContext { source: Option<anyhow::Error> },
    #[error("No auth configuration found for the profile")]
    NoAuthConfiguration {
        profile: String,
        source: Option<anyhow::Error>,
    },
    #[error("No context is selected")]
    NoContextIsSelected { source: Option<anyhow::Error> },
    #[error("No such profile")]
    NoSuchProfile {
        profile: String,
        source: Option<anyhow::Error>,
    },
    #[error("Unexpected error")]
    UnexpectedError { source: Option<anyhow::Error> },
}

#[derive(Default, Debug, PartialEq, Eq)]
pub struct Context {
    pub name: String,
    pub active: bool,
}

impl AsRef<str> for Context {
    fn as_ref(&self) -> &str {
        &self.name
    }
}
