use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use base64::Engine;
use hyprduck_engine_types::{
    EngineConfigPayload, ProviderModelCatalogResponseData, ProviderOption, ReadinessCheck,
    RuntimeReadinessResponseData, ValidateProviderResponseData, ValidationIssue,
};
use reqwest::{blocking::Client, Url};

pub(crate) mod catalog;
pub(crate) mod config;
pub(crate) mod openai_compatible;
pub(crate) mod parse_provider;
pub(crate) mod readiness;

pub(crate) use catalog::*;
pub(crate) use config::*;
pub(crate) use parse_provider::*;
pub(crate) use readiness::*;
