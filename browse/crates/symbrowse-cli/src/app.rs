//! Cobra-equivalent command execution for the Rust `symbrowse` binary.
//!
//! `run` mirrors Go's `runCLI`: resolve the command (`cobra::find`), parse
//! flags, validate arguments, run the handler and render errors exactly as
//! `exitcodes.FormatCLIError` / `output.WriteError` do.

use std::{
    io::{Read, Write},
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

use serde_json::Value;
use symbrowse_core::{
    error::ErrorCode,
    go_value,
    output::{Envelope, Format, Warning},
};
use symbrowse_daemon::{Client, ClientError, ClientOptions, Frame, Response, default_socket_path};

use crate::cobra::{self, ArgSpec, FlagValues, Tree};

mod frames;

pub use frames::*;

static TREE: OnceLock<Tree> = OnceLock::new();

pub fn tree() -> &'static Tree {
    TREE.get_or_init(Tree::load)
}

/// Mirror of `*exitcodes.CLIError` plus the structured-error interfaces the
/// Go `output.FromError` consults.
#[derive(Clone, Debug, Default)]
pub struct CliError {
    /// `err.Error()` of the outermost error that is not a CLIError wrapper,
    /// or the CLIError message.
    pub message: String,
    pub cause: Option<String>,
    pub exit: u8,
    pub is_cli: bool,
    pub kind_code: Option<ErrorCode>,
    pub hint: String,
    /// `ErrorCode()/ErrorHint()/ErrorDetails()` of a wrapped daemon error.
    pub coded: Option<Coded>,
}

#[derive(Clone, Debug)]
pub struct Coded {
    pub code: ErrorCode,
    pub hint: String,
    pub details: Option<Value>,
}

impl CliError {
    /// A plain `errors.New`/`fmt.Errorf` error.
    pub fn plain(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            exit: 1,
            ..Self::default()
        }
    }

    /// `invalidArgs(...)`: validation error with exit code 2.
    pub fn invalid_args(message: impl Into<String>) -> Self {
        Self::wrapf(2, ErrorCode::InvalidArgs, message)
    }

    /// `exitcodes.Wrapf(nil, code, kind, ...)`.
    pub fn wrapf(exit: u8, code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            exit,
            is_cli: true,
            kind_code: Some(code),
            ..Self::default()
        }
    }

    /// `err.Error()`.
    pub fn error_string(&self) -> String {
        match (&self.cause, self.is_cli) {
            (Some(cause), true) => format!("{}: {cause}", self.message),
            _ => self.message.clone(),
        }
    }

    /// `exitcodes.FormatCLIError`.
    pub fn format(&self) -> String {
        let message = self.error_string();
        if self.is_cli && !self.hint.is_empty() {
            format!("{message}\nHint: {}", self.hint)
        } else {
            message
        }
    }

    /// `output.FromError`.
    pub fn envelope(&self) -> Envelope {
        let mut envelope;
        if let Some(coded) = &self.coded {
            envelope = Envelope::failure(coded.code, self.error_string());
            let hint = if coded.hint.is_empty() {
                self.hint.clone()
            } else {
                coded.hint.clone()
            };
            if let Some(error) = envelope.error.as_mut() {
                error.hint = hint;
                error.details = coded
                    .details
                    .clone()
                    .filter(|d| !d.as_object().is_some_and(serde_json::Map::is_empty));
            }
        } else if self.is_cli {
            let code = code_from_kind(self.kind_code.unwrap_or(ErrorCode::Internal));
            envelope = Envelope::failure(code, self.message.clone());
            if let Some(error) = envelope.error.as_mut() {
                error.hint = self.hint.clone();
            }
        } else {
            envelope = Envelope::failure(ErrorCode::Internal, self.message.clone());
        }
        envelope
    }
}

/// `codeFromKind(kind)`: collapse a unified code to its kind's canonical code.
fn code_from_kind(code: ErrorCode) -> ErrorCode {
    use symbrowse_core::error::ErrorKind as Kind;
    match code.kind() {
        Kind::NotFound => ErrorCode::NotFound,
        Kind::Auth => ErrorCode::Auth,
        Kind::Permission => ErrorCode::Permission,
        Kind::Validation => ErrorCode::InvalidArgs,
        Kind::Config => ErrorCode::Config,
        Kind::Conflict => ErrorCode::Conflict,
        Kind::Unavailable => ErrorCode::Unavailable,
        Kind::Internal => ErrorCode::Internal,
    }
}

/// How a handler finished.
pub enum Outcome {
    Done,
    Fail(CliError),
    /// The handler already rendered its output on the process streams.
    Raw(std::process::ExitCode),
}

impl From<CliError> for Outcome {
    fn from(error: CliError) -> Self {
        Self::Fail(error)
    }
}

pub type Handled = Result<(), CliError>;

/// Execution context handed to every handler.
pub struct Ctx<'a> {
    pub out: Vec<u8>,
    pub err: Vec<u8>,
    pub flags: FlagValues,
    pub args: Vec<String>,
    pub path: Vec<String>,
    pub version: &'a str,
    pub stdin: &'a mut dyn Read,
    /// True while a batch item runs (process-stream handlers are refused).
    pub captured: bool,
}

impl Ctx<'_> {
    pub fn print(&mut self, text: &str) {
        self.out.extend_from_slice(text.as_bytes());
    }

    pub fn println(&mut self, text: &str) {
        self.print(text);
        self.out.push(b'\n');
    }

    /// `network har` defines a local `--output` that shadows the root flag.
    fn root_output(&self) -> String {
        if self.path.iter().map(String::as_str).eq(["network", "har"]) {
            return "text".into();
        }
        self.flags.string("output", "text")
    }

    /// `resolveOutputFormat`.
    pub fn format(&self) -> Result<Format, CliError> {
        resolve_format(self.flags.bool("json"), &self.root_output())
    }

    /// `structuredOutput`.
    pub fn structured(&self) -> bool {
        self.format().is_ok_and(|format| format != Format::Text)
    }

    /// `writeEnvelope`.
    pub fn write_envelope(&mut self, envelope: &Envelope) -> Handled {
        let format = self.format()?;
        let text = envelope
            .render(format)
            .map_err(|error| CliError::plain(error.to_string()))?;
        self.print(&text);
        Ok(())
    }

    /// `writeEnvelope(output.OK(data, nil))`.
    pub fn write_ok(&mut self, data: Value) -> Handled {
        self.write_envelope(&Envelope::ok(data, Vec::new()))
    }

    /// `writeEnvelopeFromResponse`.
    pub fn write_response(&mut self, response: &Response) -> Handled {
        if !response.success {
            return Err(response_error(response));
        }
        let warnings = response
            .warnings
            .iter()
            .map(|warning| Warning {
                kind: warning.kind.clone(),
                severity: warning.severity.clone(),
                message: warning.message.clone(),
                r#ref: warning.r#ref.clone(),
                excerpt: warning.excerpt.clone(),
            })
            .collect();
        let data = response
            .data
            .as_ref()
            .map_or(Value::Null, go_value::to_go_any);
        self.write_envelope(&Envelope::ok(data, warnings))
    }
}

pub fn resolve_format(json: bool, output: &str) -> Result<Format, CliError> {
    if json {
        return Ok(Format::Json);
    }
    match output {
        "text" => Ok(Format::Text),
        "json" => Ok(Format::Json),
        "yaml" => Ok(Format::Yaml),
        other => Err(CliError::invalid_args(format!(
            "invalid --output format {}: want text, json or yaml",
            symbrowse_core::go_quote::quote(other)
        ))),
    }
}

/// `responseError`.
pub fn response_error(response: &Response) -> CliError {
    let Some(error) = &response.error else {
        return CliError::plain("daemon request failed");
    };
    let parsed: Option<ErrorCode> = serde_json::from_value(Value::String(error.code.clone())).ok();
    let code = parsed.unwrap_or(ErrorCode::Internal);
    CliError {
        message: error.message.clone(),
        cause: Some(error.message.clone()),
        exit: code.exit_code(),
        is_cli: true,
        kind_code: Some(code),
        hint: error.hint.clone(),
        coded: parsed.map(|code| Coded {
            code,
            hint: error.hint.clone(),
            details: error.details.clone(),
        }),
    }
}

/// A transport-level daemon failure (`*daemon.TransportError`).
pub fn transport_error(error: &ClientError) -> CliError {
    match error {
        ClientError::Transport(daemon) => {
            let parsed: Option<ErrorCode> =
                serde_json::from_value(Value::String(daemon.code.clone())).ok();
            CliError {
                message: daemon.message.clone(),
                exit: 1,
                coded: parsed.map(|code| Coded {
                    code,
                    hint: daemon.hint.clone(),
                    details: daemon.details.clone(),
                }),
                ..CliError::default()
            }
        }
        other => CliError::plain(other.to_string()),
    }
}

static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn next_request_id() -> String {
    (REQUEST_COUNTER.fetch_add(1, Ordering::SeqCst) + 1).to_string()
}

fn valid_session(session: &str) -> bool {
    let bytes = session.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// `daemon.SocketPath(session)` validation.
fn socket_path(session: &str) -> Result<std::path::PathBuf, CliError> {
    if !valid_session(session) {
        return Err(CliError::plain(format!(
            "invalid session {}: use 1-64 letters, digits, '.', '_' or '-'",
            symbrowse_core::go_quote::quote(session)
        )));
    }
    Ok(default_socket_path(session))
}

fn client(session: &str) -> Result<Client, CliError> {
    Ok(Client::new(ClientOptions {
        socket_path: socket_path(session)?,
        session: session.to_owned(),
        ..ClientOptions::default()
    }))
}

/// `daemonRequestBudget`.
pub fn daemon_request(
    session: &str,
    command: &str,
    args: Option<Value>,
    max_tokens: Option<i64>,
) -> Result<Response, CliError> {
    let client = client(session)?;
    client
        .request(Frame {
            cmd: command.to_owned(),
            args,
            session: session.to_owned(),
            request_id: next_request_id(),
            max_tokens,
            retrieval_surface: "cli".into(),
        })
        .map_err(|error| transport_error(&error))
}

/// `requestNoAutostart`.
pub fn request_no_autostart(session: &str, command: &str) -> Result<Response, CliError> {
    let client = client(session)?;
    client
        .request_without_autostart(Frame {
            cmd: command.to_owned(),
            session: session.to_owned(),
            request_id: next_request_id(),
            ..Frame::default()
        })
        .map_err(|error| transport_error(&error))
}

/// `client.Request(Frame{Cmd, Args, Session})` without request id.
pub fn plain_request(
    session: &str,
    command: &str,
    args: Option<Value>,
) -> Result<Response, CliError> {
    let client = client(session)?;
    client
        .request(Frame {
            cmd: command.to_owned(),
            args,
            session: session.to_owned(),
            ..Frame::default()
        })
        .map_err(|error| transport_error(&error))
}

// ---------------------------------------------------------------------------
// Execution

pub struct Execution {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit: u8,
    pub raw: Option<std::process::ExitCode>,
}

/// Result of resolving and running a command line without error rendering.
struct RunFailure {
    error: CliError,
    flags: FlagValues,
    path: Vec<String>,
}

fn normalize(error: CliError) -> CliError {
    if error.is_cli {
        return error;
    }
    let message = &error.message;
    if message.contains("arg(s)")
        || message.starts_with("unknown command ")
        || message.starts_with("unknown flag ")
        || message.contains("flag provided but not defined")
    {
        return CliError::invalid_args(message.clone());
    }
    error
}

/// Runs `args` as `runCLI` does and returns the captured streams.
pub fn run(args: &[String], version: &str, stdin: &mut dyn Read) -> Execution {
    let mut ctx_out = Vec::new();
    let mut ctx_err = Vec::new();
    let result = execute(args, version, stdin, false, &mut ctx_out, &mut ctx_err);
    match result {
        Ok(Outcome::Raw(code)) => Execution {
            stdout: ctx_out,
            stderr: ctx_err,
            exit: 0,
            raw: Some(code),
        },
        Ok(_) => Execution {
            stdout: ctx_out,
            stderr: ctx_err,
            exit: 0,
            raw: None,
        },
        Err(failure) => {
            let error = normalize(failure.error);
            let structured = resolve_format(
                failure.flags.bool("json"),
                &if failure
                    .path
                    .iter()
                    .map(String::as_str)
                    .eq(["network", "har"])
                {
                    "text".to_owned()
                } else {
                    failure.flags.string("output", "text")
                },
            )
            .is_ok_and(|format| format != Format::Text);
            if structured {
                let format = resolve_format(
                    failure.flags.bool("json"),
                    &failure.flags.string("output", "text"),
                )
                .unwrap_or(Format::Text);
                if let Ok(text) = error.envelope().render(format) {
                    ctx_out.extend_from_slice(text.as_bytes());
                }
            } else {
                ctx_err.extend_from_slice(error.format().as_bytes());
                ctx_err.push(b'\n');
            }
            Execution {
                stdout: ctx_out,
                stderr: ctx_err,
                exit: error.exit,
                raw: None,
            }
        }
    }
}

/// Batch item runner: `root.ExecuteContext` result without normalisation.
pub fn run_item(args: &[String], version: &str) -> (Vec<u8>, Option<CliError>) {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let mut stdin = std::io::stdin();
    match execute(args, version, &mut stdin, true, &mut out, &mut err) {
        Ok(Outcome::Fail(error)) => (out, Some(error)),
        Ok(_) => (out, None),
        Err(failure) => (out, Some(failure.error)),
    }
}

fn strings_of(args: &[String]) -> Vec<String> {
    args.to_vec()
}

fn execute(
    args: &[String],
    version: &str,
    stdin: &mut dyn Read,
    captured: bool,
    out: &mut Vec<u8>,
    err: &mut Vec<u8>,
) -> Result<Outcome, RunFailure> {
    let tree = tree();
    let found = cobra::find(tree, &strings_of(args));
    let fail =
        |error: CliError, flags: FlagValues, path: Vec<String>| RunFailure { error, flags, path };
    let path = tree.nodes[found.index].path.clone();
    if let Some(message) = found.legacy_error {
        return Err(fail(CliError::plain(message), FlagValues::default(), path));
    }
    let flagset = tree.flagset(found.index);
    let parsed = match cobra::parse_flags(&flagset, &found.args) {
        Ok(parsed) => parsed,
        Err(failure) => {
            return Err(fail(CliError::plain(failure.message), failure.state, path));
        }
    };
    let node = &tree.nodes[found.index];
    if parsed.values.bool("help") {
        out.extend_from_slice(node.help.as_bytes());
        return Ok(Outcome::Done);
    }
    if path.is_empty() && parsed.values.bool("version") {
        out.extend_from_slice(symbrowse_protocol::render_root_version(version).as_bytes());
        return Ok(Outcome::Done);
    }
    if !cobra::runnable(tree, found.index) {
        out.extend_from_slice(node.help.as_bytes());
        return Ok(Outcome::Done);
    }
    let spec = cobra::args_validator(&path);
    if let Err(message) = cobra::validate_args(spec, tree, found.index, &parsed.args) {
        return Err(fail(CliError::plain(message), parsed.values, path));
    }
    if let Err(error) = frames::custom_args(&path, &parsed.values, &parsed.args) {
        return Err(fail(error, parsed.values, path));
    }
    let mut ctx = Ctx {
        out: Vec::new(),
        err: Vec::new(),
        flags: parsed.values,
        args: parsed.args,
        path: path.clone(),
        version,
        stdin,
        captured,
    };
    // PersistentPreRunE: resolveOutputFormat(cmd).
    if let Err(error) = ctx.format() {
        return Err(fail(error, ctx.flags, path));
    }
    let outcome = frames::dispatch(&mut ctx);
    out.extend_from_slice(&ctx.out);
    err.extend_from_slice(&ctx.err);
    match outcome {
        Outcome::Fail(error) => Err(fail(error, ctx.flags, path)),
        other => Ok(other),
    }
}

/// Shared by handlers: exact-args check for closure validators.
pub fn arg_error(
    spec: ArgSpec,
    tree: &Tree,
    index: usize,
    args: &[String],
) -> Result<(), CliError> {
    cobra::validate_args(spec, tree, index, args).map_err(CliError::plain)
}
