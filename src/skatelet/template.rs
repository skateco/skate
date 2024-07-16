use std::error::Error;
use std::io;
use std::ops::Deref;
use anyhow::anyhow;
use clap::{Args, Subcommand};
use handlebars::{Context, Handlebars, handlebars_helper, Helper, Output, RenderContext, RenderError, RenderErrorReason, ScopedJson};
use serde_json::Value;
use crate::skatelet::apply::{ApplyArgs, StdinCommand};


#[derive(Debug, Subcommand)]
pub enum StdinJsonCommand {
    #[command(name = "-", about = "pipe json via stdin")]
    Stdin {},
}

#[derive(Debug, Args)]
pub struct TemplateArgs {
    #[arg(short, long, long_help("The file to template."))]
    file: String,
    #[command(subcommand)]
    command: StdinJsonCommand,
}

pub fn template(template_args: TemplateArgs) -> Result<(), Box<dyn Error>> {
    let manifest = match template_args.command {
        StdinJsonCommand::Stdin {} => Ok(()),
        _ => Err(anyhow!("must give '-' to use stdin as input"))
    }?;

    let mut handlebars = Handlebars::new();
    handlebars.set_strict_mode(true);

    handlebars.register_template_file(&template_args.file, &template_args.file).map_err(|e| anyhow!(e).context("failed to load template file"))?;

    let json: Value = serde_json::from_reader(io::stdin()).map_err(|e| anyhow!(e).context("failed to parse stdin"))?;

    let output = handlebars.render(&template_args.file, &json)?;
    println!("{}", output);

    Ok(())
}
