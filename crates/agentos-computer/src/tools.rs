//! Computer-control tools.
//!
//! Every one of them is scoped to the application that is in front when the call
//! is planned, as a [`ResourceRef::Application`]. That is what lets a policy say
//! "this agent may type in Mail" rather than "this agent may type", and it is
//! the whole of what scoping can achieve here: it binds *who receives* an event,
//! never *what the event does*.
//!
//! Three refusals are built in rather than left to policy.
//!
//! * **No focused application is a refusal, not an unscoped call.** A capability
//!   with no resource is matched by any unscoped rule, so "I could not tell what
//!   was in front" must never become "…so allow it".
//! * **AgentOS is never a target.** An agent that can click can click Approve.
//!   The policy engine cannot express that — its immutable denies have no
//!   resource dimension — so the tool refuses it directly.
//! * **Focus is re-read before every event.** Authorisation happens before
//!   execution, and on the paths that matter most a human is in between, so by
//!   the time an event is sent the check is old. The backend repeats it.

use std::path::PathBuf;
use std::sync::Arc;

use agentos_core::permission::{Capability, ResourceRef, permission_domains};
use agentos_core::risk::RiskLevel;
use agentos_core::tool::ToolMetadata;
use agentos_core::trust::{DataSource, UntrustedImage};
use agentos_tools::{
    Tool, ToolContext, ToolError, ToolOutput, ToolPlan, metadata_for, parse_arguments,
};
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

use crate::desktop::{Desktop, FocusedApplication};
use crate::error::ComputerError;
use crate::input::{Axis, Button, InputAction, Key, Modifier, Point};

/// The capability action that permits sending pixels to a model.
///
/// Separate from `screenshot` because they are different acts with different
/// blast radii: one writes a file the operator owns, the other transmits the
/// contents of their screen to somebody else's server. A policy that granted the
/// first before this existed does not silently acquire the second.
const VISION_ACTION: &str = "vision";

/// Longest string `computer.type` will enter in one call.
///
/// Every character is a separate event with its own focus check, so a long
/// string is a long time spent exposed. It is also a cap on how much an approval
/// card has to show before the operator stops reading it.
pub const MAX_TYPE_CHARS: usize = 2000;

/// Most presses one `computer.click` may make.
pub const MAX_CLICK_COUNT: u8 = 3;

/// The application in front, refusing the cases that must not become a grant.
///
/// Runs during planning, so it only reads.
fn frontmost(desktop: &dyn Desktop) -> Result<FocusedApplication, ComputerError> {
    let application = desktop.focused()?;
    if application.pid == std::process::id() {
        return Err(ComputerError::SelfTargeted);
    }
    Ok(application)
}

/// Check that the application the call named is the one that will receive it.
///
/// The name is an argument rather than something the tool discovers, so that the
/// policy, the approval card and the event all describe the same target. A tool
/// that resolved the target for itself at execution time would faithfully type
/// the message it was authorised to send to Mail into whatever had taken focus
/// in the meantime.
fn require_in_front(desktop: &dyn Desktop, requested: &str) -> Result<(), ComputerError> {
    let actual = frontmost(desktop)?;
    if actual.name == requested {
        Ok(())
    } else {
        Err(ComputerError::NotInFront {
            requested: requested.to_owned(),
            actual: actual.name,
        })
    }
}

fn application_capability(action: &str, application: &str) -> Capability {
    Capability::new(permission_domains::COMPUTER, action).with_resource(ResourceRef::Application {
        application: application.to_owned(),
    })
}

/// Refuse a coordinate that is not on any display.
///
/// Not a security control — a click at (0, 0) is as dangerous as one at
/// (2000, 30) — but a model that has miscalculated an offset should be told so
/// rather than have the event land at the edge of the screen.
fn on_screen(desktop: &dyn Desktop, point: Point) -> Result<(), ComputerError> {
    let displays = desktop.displays()?;
    let contained = displays.iter().any(|display| {
        let within_x = point.x >= display.origin.x
            && point.x < display.origin.x.saturating_add_unsigned(display.width);
        let within_y = point.y >= display.origin.y
            && point.y < display.origin.y.saturating_add_unsigned(display.height);
        within_x && within_y
    });
    if contained {
        Ok(())
    } else {
        Err(ComputerError::OffScreen {
            x: point.x,
            y: point.y,
        })
    }
}

/// Plan an input action: name the target, price the risk, describe the act.
fn plan_input(
    desktop: &dyn Desktop,
    action_name: &str,
    application: &str,
    action: &InputAction,
    baseline: RiskLevel,
) -> Result<ToolPlan, ToolError> {
    require_in_front(desktop, application).map_err(ToolError::from)?;
    let risk = if action.commits() {
        RiskLevel::Critical
    } else {
        baseline
    };

    let mut summary = format!("In {application}: {action}");
    if action.commits() {
        summary.push_str(" — this commits what has been typed");
    }
    let events = action.event_count();
    if events > 1 {
        summary.push_str(&format!(" ({events} events)"));
    }

    Ok(ToolPlan::new(risk, summary).requiring(application_capability(action_name, application)))
}

/// Run an action on a blocking thread, scoped to the application that was named.
///
/// The backend re-checks focus before each event it sends, so this is the last
/// point at which the target is a name and the first at which it is a fact.
async fn perform(
    desktop: &Arc<dyn Desktop>,
    action: InputAction,
    application: String,
    cancel: &CancellationToken,
) -> Result<String, ToolError> {
    if cancel.is_cancelled() {
        return Err(ToolError::Cancelled);
    }
    let described = action.to_string();
    let desktop = Arc::clone(desktop);
    let expected = application.clone();
    // Handed through rather than only checked here: typing is one event per
    // character, and a cancelled run should stop between characters rather than
    // finish the sentence.
    let cancel = cancel.clone();
    tokio::task::spawn_blocking(move || desktop.perform(&action, &expected, &cancel))
        .await
        .map_err(|error| ToolError::Failed(error.to_string()))?
        .map_err(ToolError::from)?;

    Ok(format!("Sent to {application}: {described}"))
}

/// Arguments for `computer.inspect`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InspectArgs {}

/// Reports what is on the desktop.
#[derive(Debug)]
pub struct Inspect {
    metadata: ToolMetadata,
    desktop: Arc<dyn Desktop>,
}

impl Inspect {
    /// Build the tool.
    #[must_use]
    pub fn new(desktop: Arc<dyn Desktop>) -> Self {
        Self {
            metadata: metadata_for::<InspectArgs>(
                "computer.inspect",
                "List the windows and displays, and say which application is in front. Use this \
                 before any other computer tool: everything else is scoped to the application \
                 that has focus.",
                RiskLevel::Low,
                vec![Capability::new(permission_domains::COMPUTER, "read")],
                true,
            ),
            desktop,
        }
    }
}

#[async_trait]
impl Tool for Inspect {
    fn metadata(&self) -> &ToolMetadata {
        &self.metadata
    }

    fn validate(&self, arguments: &serde_json::Value) -> Result<serde_json::Value, ToolError> {
        let _: InspectArgs = parse_arguments(&self.metadata.name, arguments)?;
        Ok(arguments.clone())
    }

    async fn plan(
        &self,
        _arguments: &serde_json::Value,
        _context: &ToolContext,
    ) -> Result<ToolPlan, ToolError> {
        Ok(
            ToolPlan::new(RiskLevel::Low, "List the windows and displays")
                .requiring(Capability::new(permission_domains::COMPUTER, "read")),
        )
    }

    async fn execute(
        &self,
        _arguments: serde_json::Value,
        _context: &ToolContext,
        _cancel: CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        let desktop = Arc::clone(&self.desktop);
        let (windows, displays, cursor) = tokio::task::spawn_blocking(move || {
            let windows = desktop.windows()?;
            let displays = desktop.displays()?;
            let cursor = desktop.cursor()?;
            Ok::<_, ComputerError>((windows, displays, cursor))
        })
        .await
        .map_err(|error| ToolError::Failed(error.to_string()))?
        .map_err(ToolError::from)?;

        let mut report = String::new();
        report.push_str("Displays (sizes in points; a capture is `scale` times larger):\n");
        for display in &displays {
            report.push_str(&format!(
                "  {} {}x{} at {} scale {}{}\n",
                display.name,
                display.width,
                display.height,
                display.origin,
                display.scale,
                if display.primary { " (primary)" } else { "" }
            ));
        }
        report.push_str(&format!("Cursor: {cursor}\n"));
        report.push_str("Windows:\n");
        for window in &windows {
            report.push_str(&format!(
                "  {}{} — {} at {} {}x{}\n",
                window.application,
                if window.focused { " (in front)" } else { "" },
                window.title,
                window.origin,
                window.width,
                window.height
            ));
        }

        let structured = serde_json::json!({
            "cursor": {"x": cursor.x, "y": cursor.y},
            "displays": displays.iter().map(|display| serde_json::json!({
                "name": display.name,
                "width": display.width,
                "height": display.height,
                "x": display.origin.x,
                "y": display.origin.y,
                "scale": display.scale,
                "primary": display.primary,
            })).collect::<Vec<_>>(),
            "windows": windows.iter().map(|window| serde_json::json!({
                "application": window.application,
                "title": window.title,
                "focused": window.focused,
                "minimised": window.minimised,
                "x": window.origin.x,
                "y": window.origin.y,
                "width": window.width,
                "height": window.height,
            })).collect::<Vec<_>>(),
        });

        // Window titles are content. A document called "ignore your previous
        // instructions" is a document somebody named that.
        Ok(ToolOutput::text(
            DataSource::Screen {
                target: "desktop".to_owned(),
            },
            report,
        )
        .with_structured(structured))
    }
}

/// What a screenshot captures.
#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CaptureTarget {
    /// Only the window in front. Narrower, and scopable by application.
    #[default]
    Window,
    /// A whole display, including every other application on it.
    Display,
}

/// Arguments for `computer.screenshot`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScreenshotArgs {
    /// Filename to write inside the agent's workspace. Omit to capture without
    /// keeping a copy on disk, which only makes sense alongside `attach`.
    #[serde(default)]
    pub filename: Option<String>,
    /// What to capture. Defaults to the window in front.
    #[serde(default)]
    pub target: CaptureTarget,
    /// For a window capture, the application that must be in front. Required
    /// there, and refused for a display capture, which is nobody's window.
    #[serde(default)]
    pub application: Option<String>,
    /// Whether to show the capture to the model.
    ///
    /// Off unless asked for. Saving a screenshot puts pixels on the operator's
    /// own disk; showing one to a model sends them to a third party, and an
    /// existing policy that permitted the first must not start permitting the
    /// second because the runtime was upgraded.
    #[serde(default)]
    pub attach: bool,
}

/// Captures the screen as a PNG.
#[derive(Debug)]
pub struct Screenshot {
    metadata: ToolMetadata,
    desktop: Arc<dyn Desktop>,
}

impl Screenshot {
    /// Build the tool.
    #[must_use]
    pub fn new(desktop: Arc<dyn Desktop>) -> Self {
        Self {
            metadata: metadata_for::<ScreenshotArgs>(
                "computer.screenshot",
                "Capture the window in front, or a whole display. Set `filename` to save a PNG \
                 into the agent's workspace, set `attach` to be shown the image, or both. A \
                 display capture includes every other application visible on it.",
                RiskLevel::Medium,
                vec![
                    Capability::new(permission_domains::COMPUTER, "screenshot"),
                    Capability::new(permission_domains::COMPUTER, VISION_ACTION),
                    Capability::new(permission_domains::FILESYSTEM, "write"),
                ],
                true,
            ),
            desktop,
        }
    }

    /// Resolve the output path inside the workspace.
    ///
    /// The same resolution every other write goes through, so a filename of
    /// `../../.ssh/authorized_keys` is caught here rather than trusted because
    /// it arrived through a screenshot tool.
    fn destination(
        &self,
        filename: Option<&String>,
        context: &ToolContext,
    ) -> Result<Option<PathBuf>, ToolError> {
        let Some(filename) = filename else {
            return Ok(None);
        };
        let candidate = context.workspace.join(filename);
        agentos_permissions::path::resolve_secure(&candidate)
            .map(Some)
            .map_err(ToolError::Path)
    }
}

#[async_trait]
impl Tool for Screenshot {
    fn metadata(&self) -> &ToolMetadata {
        &self.metadata
    }

    fn validate(&self, arguments: &serde_json::Value) -> Result<serde_json::Value, ToolError> {
        let args: ScreenshotArgs = parse_arguments(&self.metadata.name, arguments)?;
        if args
            .filename
            .as_ref()
            .is_some_and(|name| name.trim().is_empty())
        {
            return Err(ToolError::invalid(
                &self.metadata.name,
                "`filename` must not be empty",
            ));
        }
        // A capture that is neither saved nor shown is a capture for nobody. It
        // would still read the screen, so it is refused rather than run.
        if args.filename.is_none() && !args.attach {
            return Err(ToolError::invalid(
                &self.metadata.name,
                "set `filename` to save the capture, `attach` to be shown it, or both",
            ));
        }
        match (args.target, &args.application) {
            (CaptureTarget::Window, None) => Err(ToolError::invalid(
                &self.metadata.name,
                "capturing a window needs `application`; call `computer.inspect` for the name",
            )),
            (CaptureTarget::Display, Some(_)) => Err(ToolError::invalid(
                &self.metadata.name,
                "a display capture is not scoped to an application, so `application` does not apply",
            )),
            _ => Ok(arguments.clone()),
        }
    }

    async fn plan(
        &self,
        arguments: &serde_json::Value,
        context: &ToolContext,
    ) -> Result<ToolPlan, ToolError> {
        let args: ScreenshotArgs = parse_arguments(&self.metadata.name, arguments)?;
        let destination = self.destination(args.filename.as_ref(), context)?;

        // A window capture can be scoped to the application; a display capture
        // cannot be scoped to anything, because a display is not one
        // application's. That asymmetry is the point of offering both.
        let (capability, vision_capability, what) = match (args.target, args.application.clone()) {
            (CaptureTarget::Window, Some(application)) => {
                require_in_front(self.desktop.as_ref(), &application).map_err(ToolError::from)?;
                (
                    application_capability("screenshot", &application),
                    application_capability(VISION_ACTION, &application),
                    format!("the {application} window"),
                )
            }
            (CaptureTarget::Window, None) => {
                return Err(ToolError::invalid(
                    &self.metadata.name,
                    "capturing a window needs `application`",
                ));
            }
            (CaptureTarget::Display, _) => (
                Capability::new(permission_domains::COMPUTER, "screenshot"),
                Capability::new(permission_domains::COMPUTER, VISION_ACTION),
                "the whole display, including every other application on it".to_owned(),
            ),
        };

        // Sending a whole display to a third party is the broadest egress in the
        // system: every window on it, including the ones the agent was never
        // granted and the ones the operator forgot were open.
        let risk = match (args.attach, args.target) {
            (true, CaptureTarget::Display) => RiskLevel::High,
            _ => RiskLevel::Medium,
        };

        let summary = match (&destination, args.attach) {
            (Some(path), true) => format!(
                "Capture {what}, show it to the model, and save it to {}",
                path.display()
            ),
            (Some(path), false) => format!("Capture {what} to {}", path.display()),
            (None, _) => format!("Capture {what} and show it to the model"),
        };

        let mut plan = ToolPlan::new(risk, summary).requiring(capability);
        if args.attach {
            plan = plan.requiring(vision_capability);
        }
        if let Some(path) = &destination {
            plan = plan.requiring(
                Capability::new(permission_domains::FILESYSTEM, "write").with_resource(
                    ResourceRef::Path {
                        path: path.display().to_string(),
                    },
                ),
            );
        }
        Ok(plan)
    }

    async fn execute(
        &self,
        arguments: serde_json::Value,
        context: &ToolContext,
        _cancel: CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        let args: ScreenshotArgs = parse_arguments(&self.metadata.name, &arguments)?;
        let destination = self.destination(args.filename.as_ref(), context)?;

        let desktop = Arc::clone(&self.desktop);
        let target = args.target;
        let wanted = args.application.clone();
        let capture = tokio::task::spawn_blocking(move || match (target, wanted) {
            (CaptureTarget::Window, Some(application)) => {
                // The same check the input tools make, for the same reason: an
                // approval to capture the Mail window must not produce a capture
                // of whatever took focus while the operator was reading it.
                require_in_front(desktop.as_ref(), &application)?;
                let capture = desktop.capture_focused()?;
                if capture.target == application {
                    Ok(capture)
                } else {
                    Err(ComputerError::NotInFront {
                        requested: application,
                        actual: capture.target,
                    })
                }
            }
            (CaptureTarget::Window, None) => Err(ComputerError::NoFocusedApplication),
            (CaptureTarget::Display, _) => desktop.capture_display(None),
        })
        .await
        .map_err(|error| ToolError::Failed(error.to_string()))?
        .map_err(ToolError::from)?;

        let bytes = capture.png.len();
        let source = DataSource::Screen {
            target: capture.target.clone(),
        };

        // The file on disk is the capture exactly as the screen was. Only what
        // goes to the model is resized, so the operator's own copy is never the
        // lossy one.
        if let Some(destination) = &destination {
            if let Some(parent) = destination.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|source| ToolError::io("creating the screenshot directory", source))?;
            }
            tokio::fs::write(destination, &capture.png)
                .await
                .map_err(|source| ToolError::io("writing the screenshot", source))?;
        }

        let attached = if args.attach {
            let prepared = agentos_tools::vision::prepare(
                &capture.png,
                context.max_image_edge,
                context.max_image_bytes,
            )
            .map_err(|error| ToolError::Failed(error.to_string()))?;
            Some(prepared)
        } else {
            None
        };

        let saved = destination
            .as_ref()
            .map(|path| format!(" and saved {bytes} bytes to {}", path.display()))
            .unwrap_or_default();
        let shown = match &attached {
            Some(prepared) if prepared.resized => format!(
                " The image below is the same capture scaled to {}x{} to fit the model's limits.",
                prepared.width, prepared.height
            ),
            Some(_) => " The image below is that capture.".to_owned(),
            None => String::new(),
        };

        let mut output = ToolOutput::text(
            source.clone(),
            format!(
                "Captured {} at {}x{} pixels{saved}.{shown} Coordinates for the other computer \
                 tools are in points, not in this image's pixels — call `computer.inspect` for \
                 the scale factor.",
                capture.target, capture.pixel_width, capture.pixel_height,
            ),
        )
        .with_structured(serde_json::json!({
            "path": destination.as_ref().map(|path| path.display().to_string()),
            "bytes": bytes,
            "target": capture.target,
            "pixel_width": capture.pixel_width,
            "pixel_height": capture.pixel_height,
            "attached": args.attach,
        }));

        if let Some(prepared) = attached {
            output = output.with_image(UntrustedImage::new(
                source,
                prepared.format,
                prepared.data,
                prepared.width,
                prepared.height,
            ));
        }
        Ok(output)
    }
}

/// Arguments for `computer.move`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MoveArgs {
    /// The application that must be in front to receive this. Get it from
    /// `computer.inspect`.
    pub application: String,
    /// Horizontal position in points from the left of the primary display.
    pub x: i32,
    /// Vertical position in points from the top of the primary display.
    pub y: i32,
}

/// Moves the cursor.
#[derive(Debug)]
pub struct MoveCursor {
    metadata: ToolMetadata,
    desktop: Arc<dyn Desktop>,
}

impl MoveCursor {
    /// Build the tool.
    #[must_use]
    pub fn new(desktop: Arc<dyn Desktop>) -> Self {
        Self {
            metadata: metadata_for::<MoveArgs>(
                "computer.move",
                "Move the mouse cursor. Coordinates are in points from the top left of the \
                 primary display, not in screenshot pixels.",
                RiskLevel::Low,
                vec![Capability::new(permission_domains::COMPUTER, "move")],
                false,
            ),
            desktop,
        }
    }
}

#[async_trait]
impl Tool for MoveCursor {
    fn metadata(&self) -> &ToolMetadata {
        &self.metadata
    }

    fn validate(&self, arguments: &serde_json::Value) -> Result<serde_json::Value, ToolError> {
        let _: MoveArgs = parse_arguments(&self.metadata.name, arguments)?;
        Ok(arguments.clone())
    }

    async fn plan(
        &self,
        arguments: &serde_json::Value,
        _context: &ToolContext,
    ) -> Result<ToolPlan, ToolError> {
        let args: MoveArgs = parse_arguments(&self.metadata.name, arguments)?;
        let point = Point::new(args.x, args.y);
        on_screen(self.desktop.as_ref(), point).map_err(ToolError::from)?;
        plan_input(
            self.desktop.as_ref(),
            "move",
            &args.application,
            &InputAction::Move { to: point },
            RiskLevel::Low,
        )
    }

    async fn execute(
        &self,
        arguments: serde_json::Value,
        _context: &ToolContext,
        cancel: CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        let args: MoveArgs = parse_arguments(&self.metadata.name, &arguments)?;
        let message = perform(
            &self.desktop,
            InputAction::Move {
                to: Point::new(args.x, args.y),
            },
            args.application,
            &cancel,
        )
        .await?;
        Ok(ToolOutput::text(DataSource::Runtime, message))
    }
}

/// Arguments for `computer.click`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClickArgs {
    /// The application that must be in front to receive this. Get it from
    /// `computer.inspect`.
    pub application: String,
    /// Horizontal position in points. Clicks where the cursor is if absent.
    #[serde(default)]
    pub x: Option<i32>,
    /// Vertical position in points. Clicks where the cursor is if absent.
    #[serde(default)]
    pub y: Option<i32>,
    /// Which button.
    #[serde(default)]
    pub button: Button,
    /// How many presses: 1 to click, 2 to double click.
    #[serde(default = "one")]
    pub count: u8,
}

const fn one() -> u8 {
    1
}

/// Clicks a mouse button.
#[derive(Debug)]
pub struct Click {
    metadata: ToolMetadata,
    desktop: Arc<dyn Desktop>,
}

impl Click {
    /// Build the tool.
    #[must_use]
    pub fn new(desktop: Arc<dyn Desktop>) -> Self {
        Self {
            metadata: metadata_for::<ClickArgs>(
                "computer.click",
                "Click a mouse button, optionally moving to a point first. What a click does \
                 depends on what is under it, which nothing here can check — prefer the browser \
                 tools when the target is a web page.",
                RiskLevel::High,
                vec![Capability::new(permission_domains::COMPUTER, "click")],
                false,
            ),
            desktop,
        }
    }

    fn action(&self, args: &ClickArgs) -> Result<InputAction, ToolError> {
        let at = match (args.x, args.y) {
            (Some(x), Some(y)) => Some(Point::new(x, y)),
            (None, None) => None,
            _ => {
                return Err(ToolError::invalid(
                    &self.metadata.name,
                    "give both `x` and `y`, or neither",
                ));
            }
        };
        Ok(InputAction::Click {
            button: args.button,
            at,
            count: args.count,
        })
    }
}

#[async_trait]
impl Tool for Click {
    fn metadata(&self) -> &ToolMetadata {
        &self.metadata
    }

    fn validate(&self, arguments: &serde_json::Value) -> Result<serde_json::Value, ToolError> {
        let args: ClickArgs = parse_arguments(&self.metadata.name, arguments)?;
        if args.count == 0 || args.count > MAX_CLICK_COUNT {
            return Err(ToolError::invalid(
                &self.metadata.name,
                format!("`count` must be between 1 and {MAX_CLICK_COUNT}"),
            ));
        }
        self.action(&args)?;
        Ok(arguments.clone())
    }

    async fn plan(
        &self,
        arguments: &serde_json::Value,
        _context: &ToolContext,
    ) -> Result<ToolPlan, ToolError> {
        let args: ClickArgs = parse_arguments(&self.metadata.name, arguments)?;
        let action = self.action(&args)?;
        if let InputAction::Click {
            at: Some(point), ..
        } = &action
        {
            on_screen(self.desktop.as_ref(), *point).map_err(ToolError::from)?;
        }
        plan_input(
            self.desktop.as_ref(),
            "click",
            &args.application,
            &action,
            RiskLevel::High,
        )
    }

    async fn execute(
        &self,
        arguments: serde_json::Value,
        _context: &ToolContext,
        cancel: CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        let args: ClickArgs = parse_arguments(&self.metadata.name, &arguments)?;
        let action = self.action(&args)?;
        let message = perform(&self.desktop, action, args.application, &cancel).await?;
        Ok(ToolOutput::text(DataSource::Runtime, message))
    }
}

/// Arguments for `computer.drag`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DragArgs {
    /// The application that must be in front to receive this. Get it from
    /// `computer.inspect`.
    pub application: String,
    /// Where the button goes down, horizontally, in points.
    pub from_x: i32,
    /// Where the button goes down, vertically, in points.
    pub from_y: i32,
    /// Where it comes up, horizontally, in points.
    pub to_x: i32,
    /// Where it comes up, vertically, in points.
    pub to_y: i32,
    /// Which button.
    #[serde(default)]
    pub button: Button,
}

/// Drags between two points.
#[derive(Debug)]
pub struct Drag {
    metadata: ToolMetadata,
    desktop: Arc<dyn Desktop>,
}

impl Drag {
    /// Build the tool.
    #[must_use]
    pub fn new(desktop: Arc<dyn Desktop>) -> Self {
        Self {
            metadata: metadata_for::<DragArgs>(
                "computer.drag",
                "Press at one point, move, and release at another. Used for selecting text and \
                 for moving things; in a file manager it moves files.",
                RiskLevel::High,
                vec![Capability::new(permission_domains::COMPUTER, "drag")],
                false,
            ),
            desktop,
        }
    }

    fn action(args: &DragArgs) -> InputAction {
        InputAction::Drag {
            button: args.button,
            from: Point::new(args.from_x, args.from_y),
            to: Point::new(args.to_x, args.to_y),
        }
    }
}

#[async_trait]
impl Tool for Drag {
    fn metadata(&self) -> &ToolMetadata {
        &self.metadata
    }

    fn validate(&self, arguments: &serde_json::Value) -> Result<serde_json::Value, ToolError> {
        let _: DragArgs = parse_arguments(&self.metadata.name, arguments)?;
        Ok(arguments.clone())
    }

    async fn plan(
        &self,
        arguments: &serde_json::Value,
        _context: &ToolContext,
    ) -> Result<ToolPlan, ToolError> {
        let args: DragArgs = parse_arguments(&self.metadata.name, arguments)?;
        let action = Self::action(&args);
        on_screen(self.desktop.as_ref(), Point::new(args.from_x, args.from_y))
            .map_err(ToolError::from)?;
        on_screen(self.desktop.as_ref(), Point::new(args.to_x, args.to_y))
            .map_err(ToolError::from)?;
        plan_input(
            self.desktop.as_ref(),
            "drag",
            &args.application,
            &action,
            RiskLevel::High,
        )
    }

    async fn execute(
        &self,
        arguments: serde_json::Value,
        _context: &ToolContext,
        cancel: CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        let args: DragArgs = parse_arguments(&self.metadata.name, &arguments)?;
        let action = Self::action(&args);
        let message = perform(&self.desktop, action, args.application, &cancel).await?;
        Ok(ToolOutput::text(DataSource::Runtime, message))
    }
}

/// Arguments for `computer.scroll`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScrollArgs {
    /// The application that must be in front to receive this. Get it from
    /// `computer.inspect`.
    pub application: String,
    /// Wheel clicks. Positive scrolls down, or right.
    pub amount: i32,
    /// Which axis.
    #[serde(default)]
    pub axis: Axis,
}

/// Turns the scroll wheel.
#[derive(Debug)]
pub struct Scroll {
    metadata: ToolMetadata,
    desktop: Arc<dyn Desktop>,
}

impl Scroll {
    /// Build the tool.
    #[must_use]
    pub fn new(desktop: Arc<dyn Desktop>) -> Self {
        Self {
            metadata: metadata_for::<ScrollArgs>(
                "computer.scroll",
                "Scroll the application in front. Positive amounts scroll down or right.",
                RiskLevel::Low,
                vec![Capability::new(permission_domains::COMPUTER, "scroll")],
                false,
            ),
            desktop,
        }
    }
}

#[async_trait]
impl Tool for Scroll {
    fn metadata(&self) -> &ToolMetadata {
        &self.metadata
    }

    fn validate(&self, arguments: &serde_json::Value) -> Result<serde_json::Value, ToolError> {
        let _: ScrollArgs = parse_arguments(&self.metadata.name, arguments)?;
        Ok(arguments.clone())
    }

    async fn plan(
        &self,
        arguments: &serde_json::Value,
        _context: &ToolContext,
    ) -> Result<ToolPlan, ToolError> {
        let args: ScrollArgs = parse_arguments(&self.metadata.name, arguments)?;
        plan_input(
            self.desktop.as_ref(),
            "scroll",
            &args.application,
            &InputAction::Scroll {
                axis: args.axis,
                amount: args.amount,
            },
            RiskLevel::Low,
        )
    }

    async fn execute(
        &self,
        arguments: serde_json::Value,
        _context: &ToolContext,
        cancel: CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        let args: ScrollArgs = parse_arguments(&self.metadata.name, &arguments)?;
        let message = perform(
            &self.desktop,
            InputAction::Scroll {
                axis: args.axis,
                amount: args.amount,
            },
            args.application,
            &cancel,
        )
        .await?;
        Ok(ToolOutput::text(DataSource::Runtime, message))
    }
}

/// Arguments for `computer.type`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TypeArgs {
    /// The application that must be in front to receive this. Get it from
    /// `computer.inspect`.
    pub application: String,
    /// The text to enter. A newline presses Return, which usually commits.
    pub text: String,
}

/// Types text.
#[derive(Debug)]
pub struct TypeText {
    metadata: ToolMetadata,
    desktop: Arc<dyn Desktop>,
}

impl TypeText {
    /// Build the tool.
    #[must_use]
    pub fn new(desktop: Arc<dyn Desktop>) -> Self {
        Self {
            metadata: metadata_for::<TypeArgs>(
                "computer.type",
                "Type text into the application in front. A newline presses Return, which in most \
                 applications sends or confirms — that is treated as a separate, higher risk.",
                RiskLevel::High,
                vec![Capability::new(permission_domains::COMPUTER, "type")],
                false,
            ),
            desktop,
        }
    }
}

#[async_trait]
impl Tool for TypeText {
    fn metadata(&self) -> &ToolMetadata {
        &self.metadata
    }

    fn validate(&self, arguments: &serde_json::Value) -> Result<serde_json::Value, ToolError> {
        let args: TypeArgs = parse_arguments(&self.metadata.name, arguments)?;
        if args.text.is_empty() {
            return Err(ToolError::invalid(
                &self.metadata.name,
                "`text` must not be empty",
            ));
        }
        let length = args.text.chars().count();
        if length > MAX_TYPE_CHARS {
            return Err(ToolError::invalid(
                &self.metadata.name,
                format!("`text` is {length} characters; the limit is {MAX_TYPE_CHARS}"),
            ));
        }
        // A null byte is not something a keyboard can produce, and the backend
        // would refuse the whole string half-way through.
        if args.text.contains('\0') {
            return Err(ToolError::invalid(
                &self.metadata.name,
                "`text` must not contain a null byte",
            ));
        }
        Ok(arguments.clone())
    }

    async fn plan(
        &self,
        arguments: &serde_json::Value,
        _context: &ToolContext,
    ) -> Result<ToolPlan, ToolError> {
        let args: TypeArgs = parse_arguments(&self.metadata.name, arguments)?;
        plan_input(
            self.desktop.as_ref(),
            "type",
            &args.application,
            &InputAction::Type { text: args.text },
            RiskLevel::High,
        )
    }

    async fn execute(
        &self,
        arguments: serde_json::Value,
        _context: &ToolContext,
        cancel: CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        let args: TypeArgs = parse_arguments(&self.metadata.name, &arguments)?;
        let message = perform(
            &self.desktop,
            InputAction::Type { text: args.text },
            args.application,
            &cancel,
        )
        .await?;
        Ok(ToolOutput::text(DataSource::Runtime, message))
    }
}

/// Arguments for `computer.key`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KeyArgs {
    /// The application that must be in front to receive this. Get it from
    /// `computer.inspect`.
    pub application: String,
    /// The key: a single character, `f1`-`f12`, or a named key such as
    /// `escape`, `tab`, `return`, `space`, `backspace`, `delete`, `up`, `down`,
    /// `left`, `right`, `home`, `end`, `page_up`, `page_down`.
    pub key: String,
    /// Modifiers held while it is pressed.
    #[serde(default)]
    pub modifiers: Vec<Modifier>,
}

/// Presses one key, with modifiers.
#[derive(Debug)]
pub struct PressKey {
    metadata: ToolMetadata,
    desktop: Arc<dyn Desktop>,
}

impl PressKey {
    /// Build the tool.
    #[must_use]
    pub fn new(desktop: Arc<dyn Desktop>) -> Self {
        Self {
            metadata: metadata_for::<KeyArgs>(
                "computer.key",
                "Press a single key, optionally with modifiers held — the way keyboard shortcuts \
                 are invoked. Modifiers are named after the physical key, so the usual shortcut \
                 modifier is `command` on macOS and `control` on Windows.",
                RiskLevel::High,
                vec![Capability::new(permission_domains::COMPUTER, "key")],
                false,
            ),
            desktop,
        }
    }

    fn action(&self, args: &KeyArgs) -> Result<InputAction, ToolError> {
        let key = Key::parse(&args.key)
            .map_err(|message| ToolError::invalid(&self.metadata.name, message))?;
        Ok(InputAction::Key {
            key,
            modifiers: args.modifiers.clone(),
        })
    }
}

#[async_trait]
impl Tool for PressKey {
    fn metadata(&self) -> &ToolMetadata {
        &self.metadata
    }

    fn validate(&self, arguments: &serde_json::Value) -> Result<serde_json::Value, ToolError> {
        let args: KeyArgs = parse_arguments(&self.metadata.name, arguments)?;
        self.action(&args)?;
        Ok(arguments.clone())
    }

    async fn plan(
        &self,
        arguments: &serde_json::Value,
        _context: &ToolContext,
    ) -> Result<ToolPlan, ToolError> {
        let args: KeyArgs = parse_arguments(&self.metadata.name, arguments)?;
        plan_input(
            self.desktop.as_ref(),
            "key",
            &args.application,
            &self.action(&args)?,
            RiskLevel::High,
        )
    }

    async fn execute(
        &self,
        arguments: serde_json::Value,
        _context: &ToolContext,
        cancel: CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        let args: KeyArgs = parse_arguments(&self.metadata.name, &arguments)?;
        let action = self.action(&args)?;
        let message = perform(&self.desktop, action, args.application, &cancel).await?;
        Ok(ToolOutput::text(DataSource::Runtime, message))
    }
}

/// Every computer tool, sharing one backend.
#[must_use]
pub fn all(desktop: Arc<dyn Desktop>) -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(Inspect::new(Arc::clone(&desktop))),
        Arc::new(Screenshot::new(Arc::clone(&desktop))),
        Arc::new(MoveCursor::new(Arc::clone(&desktop))),
        Arc::new(Click::new(Arc::clone(&desktop))),
        Arc::new(Drag::new(Arc::clone(&desktop))),
        Arc::new(Scroll::new(Arc::clone(&desktop))),
        Arc::new(TypeText::new(Arc::clone(&desktop))),
        Arc::new(PressKey::new(desktop)),
    ]
}

#[cfg(test)]
mod tests {
    use agentos_core::ids::{AgentId, TaskId, TaskRunId};

    use super::*;
    use crate::desktop::testing::RecordingDesktop;

    fn context() -> ToolContext {
        ToolContext::new(
            AgentId::new(),
            TaskId::new(),
            TaskRunId::new(),
            std::env::temp_dir(),
        )
    }

    fn tool_named(desktop: Arc<dyn Desktop>, name: &str) -> Arc<dyn Tool> {
        all(desktop)
            .into_iter()
            .find(|tool| tool.metadata().name == name)
            .unwrap_or_else(|| panic!("no tool called {name}"))
    }

    /// The arguments each input tool needs, targeting `application`.
    fn arguments_for(tool: &str, application: &str) -> serde_json::Value {
        match tool {
            "computer.move" => serde_json::json!({"application": application, "x": 10, "y": 10}),
            "computer.click" => serde_json::json!({"application": application, "x": 10, "y": 10}),
            "computer.drag" => serde_json::json!({
                "application": application, "from_x": 1, "from_y": 1, "to_x": 2, "to_y": 2
            }),
            "computer.scroll" => serde_json::json!({"application": application, "amount": 3}),
            "computer.type" => serde_json::json!({"application": application, "text": "hello"}),
            "computer.key" => serde_json::json!({"application": application, "key": "return"}),
            other => panic!("no argument shape for {other}"),
        }
    }

    const INPUT_TOOLS: &[&str] = &[
        "computer.move",
        "computer.click",
        "computer.drag",
        "computer.scroll",
        "computer.type",
        "computer.key",
    ];

    #[tokio::test]
    async fn input_is_scoped_to_the_application_it_names() {
        let desktop: Arc<dyn Desktop> = Arc::new(RecordingDesktop::in_front("Mail"));
        for name in INPUT_TOOLS {
            let plan = tool_named(Arc::clone(&desktop), name)
                .plan(&arguments_for(name, "Mail"), &context())
                .await
                .unwrap_or_else(|error| panic!("{name} could not plan: {error}"));

            assert_eq!(
                plan.capabilities[0].resource,
                Some(ResourceRef::Application {
                    application: "Mail".to_owned()
                }),
                "{name} was not scoped to an application"
            );
            assert_eq!(plan.affected_resources, vec!["application:Mail"]);
            assert!(plan.summary.starts_with("In Mail: "), "{name}");
        }
    }

    #[tokio::test]
    async fn naming_an_application_that_is_not_in_front_is_refused() {
        // Otherwise an agent could hold an authorisation for Mail and spend it
        // on whatever happened to have focus by the time it ran.
        let desktop: Arc<dyn Desktop> = Arc::new(RecordingDesktop::in_front("Slack"));
        for name in INPUT_TOOLS {
            let error = tool_named(Arc::clone(&desktop), name)
                .plan(&arguments_for(name, "Mail"), &context())
                .await
                .unwrap_err();
            assert!(
                error.to_string().contains("`Mail` is not in front"),
                "{name}: {error}"
            );
        }
    }

    #[tokio::test]
    async fn a_call_with_nothing_in_front_is_refused_rather_than_unscoped() {
        // The failure that matters: a capability with no resource is matched by
        // any rule that lists no resources, so "I cannot tell what is in front"
        // must not quietly become "…so allow it".
        let desktop: Arc<dyn Desktop> = Arc::new(RecordingDesktop::with_nothing_in_front());
        for name in INPUT_TOOLS {
            let error = tool_named(Arc::clone(&desktop), name)
                .plan(&arguments_for(name, "Mail"), &context())
                .await
                .unwrap_err();
            assert!(
                error.to_string().contains("no application is in front"),
                "{name}: {error}"
            );
        }
    }

    #[tokio::test]
    async fn agentos_is_never_a_target() {
        // An agent that can click can click Approve. The policy engine cannot
        // express this — its immutable denies have no resource dimension — so
        // the refusal lives here.
        let desktop: Arc<dyn Desktop> =
            Arc::new(RecordingDesktop::in_front("AgentOS").owned_by_this_process());
        for name in INPUT_TOOLS {
            let error = tool_named(Arc::clone(&desktop), name)
                .plan(&arguments_for(name, "AgentOS"), &context())
                .await
                .unwrap_err();
            assert!(
                error.to_string().contains("may not send input"),
                "{name} was allowed to target AgentOS: {error}"
            );
        }
    }

    #[tokio::test]
    async fn committing_keystrokes_are_priced_above_the_rest() {
        let desktop: Arc<dyn Desktop> = Arc::new(RecordingDesktop::in_front("Mail"));

        let draft = tool_named(Arc::clone(&desktop), "computer.type")
            .plan(
                &serde_json::json!({"application": "Mail", "text": "a draft"}),
                &context(),
            )
            .await
            .unwrap();
        assert_eq!(draft.risk, RiskLevel::High);
        assert!(!draft.summary.contains("commits"));

        let send = tool_named(Arc::clone(&desktop), "computer.type")
            .plan(
                &serde_json::json!({"application": "Mail", "text": "a draft\n"}),
                &context(),
            )
            .await
            .unwrap();
        assert_eq!(send.risk, RiskLevel::Critical);
        assert!(send.summary.contains("commits"));

        let key = tool_named(Arc::clone(&desktop), "computer.key")
            .plan(
                &serde_json::json!({"application": "Mail", "key": "return"}),
                &context(),
            )
            .await
            .unwrap();
        assert_eq!(key.risk, RiskLevel::Critical);

        let copy = tool_named(desktop, "computer.key")
            .plan(
                &serde_json::json!({
                    "application": "Mail", "key": "c", "modifiers": ["command"]
                }),
                &context(),
            )
            .await
            .unwrap();
        assert_eq!(copy.risk, RiskLevel::High);
        assert!(copy.summary.contains("command-c"));
    }

    #[tokio::test]
    async fn execution_stops_when_focus_moves_after_authorisation() {
        let fake = Arc::new(RecordingDesktop::in_front("Mail"));
        let desktop: Arc<dyn Desktop> = Arc::clone(&fake) as Arc<dyn Desktop>;
        let tool = tool_named(Arc::clone(&desktop), "computer.type");
        let arguments = serde_json::json!({"application": "Mail", "text": "the password"});

        tool.plan(&arguments, &context()).await.unwrap();

        // Between authorisation and execution — where a human approval sits, and
        // where approving is itself an act of focusing another window.
        fake.switch_to("Slack");

        let error = tool
            .execute(arguments, &context(), CancellationToken::new())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("focus moved"), "{error}");
        assert!(
            fake.actions().is_empty(),
            "input was sent to the wrong window"
        );
    }

    #[tokio::test]
    async fn a_cancelled_run_sends_nothing() {
        let fake = Arc::new(RecordingDesktop::in_front("Mail"));
        let desktop: Arc<dyn Desktop> = Arc::clone(&fake) as Arc<dyn Desktop>;
        let cancel = CancellationToken::new();
        cancel.cancel();

        let error = tool_named(desktop, "computer.type")
            .execute(
                serde_json::json!({"application": "Mail", "text": "hello"}),
                &context(),
                cancel,
            )
            .await
            .unwrap_err();
        assert!(matches!(error, ToolError::Cancelled));
        assert!(fake.actions().is_empty());
    }

    #[tokio::test]
    async fn typing_is_bounded_and_validated() {
        let desktop: Arc<dyn Desktop> = Arc::new(RecordingDesktop::in_front("Mail"));
        let tool = tool_named(desktop, "computer.type");

        assert!(
            tool.validate(&serde_json::json!({"application": "Mail", "text": ""}))
                .is_err()
        );
        assert!(
            tool.validate(&serde_json::json!({"application": "Mail", "text": "\0"}))
                .is_err()
        );
        assert!(
            tool.validate(&serde_json::json!({
                "application": "Mail", "text": "x".repeat(MAX_TYPE_CHARS + 1)
            }))
            .is_err()
        );
        assert!(
            tool.validate(&serde_json::json!({
                "application": "Mail", "text": "fine", "speed": 10
            }))
            .is_err(),
            "an unknown argument must not be silently dropped"
        );
        assert!(
            tool.validate(&serde_json::json!({"text": "fine"})).is_err(),
            "the target must be named"
        );
        assert!(
            tool.validate(&serde_json::json!({"application": "Mail", "text": "fine"}))
                .is_ok()
        );
    }

    #[tokio::test]
    async fn coordinates_off_every_display_are_refused() {
        let desktop: Arc<dyn Desktop> = Arc::new(RecordingDesktop::in_front("Mail"));
        let error = tool_named(desktop, "computer.click")
            .plan(
                &serde_json::json!({"application": "Mail", "x": 9000, "y": 9000}),
                &context(),
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("is not on any display"));
    }

    #[tokio::test]
    async fn a_click_needs_both_coordinates_or_neither() {
        let desktop: Arc<dyn Desktop> = Arc::new(RecordingDesktop::in_front("Mail"));
        let tool = tool_named(desktop, "computer.click");
        assert!(
            tool.validate(&serde_json::json!({"application": "Mail", "x": 10}))
                .is_err()
        );
        assert!(
            tool.validate(&serde_json::json!({"application": "Mail", "x": 10, "y": 10}))
                .is_ok()
        );
        assert!(
            tool.validate(&serde_json::json!({"application": "Mail"}))
                .is_ok()
        );
        assert!(
            tool.validate(&serde_json::json!({
                "application": "Mail", "x": 1, "y": 1, "count": 0
            }))
            .is_err()
        );
        assert!(
            tool.validate(&serde_json::json!({
                "application": "Mail", "x": 1, "y": 1, "count": 9
            }))
            .is_err()
        );
    }

    #[tokio::test]
    async fn only_recognised_keys_are_accepted() {
        let desktop: Arc<dyn Desktop> = Arc::new(RecordingDesktop::in_front("Mail"));
        let tool = tool_named(desktop, "computer.key");
        assert!(
            tool.validate(&serde_json::json!({"application": "Mail", "key": "escape"}))
                .is_ok()
        );
        assert!(
            tool.validate(&serde_json::json!({"application": "Mail", "key": "cmd+q"}))
                .is_err(),
            "a chord must be expressed with `modifiers`"
        );
        assert!(
            tool.validate(&serde_json::json!({
                "application": "Mail", "key": "a", "modifiers": ["hyper"]
            }))
            .is_err(),
            "an unknown modifier must not be dropped"
        );
    }

    #[tokio::test]
    async fn a_window_capture_is_scoped_but_a_display_capture_cannot_be() {
        let desktop: Arc<dyn Desktop> = Arc::new(RecordingDesktop::in_front("Mail"));
        let tool = tool_named(desktop, "computer.screenshot");

        let window = tool
            .plan(
                &serde_json::json!({
                    "filename": "shot.png", "target": "window", "application": "Mail"
                }),
                &context(),
            )
            .await
            .unwrap();
        assert_eq!(
            window.capabilities[0].resource,
            Some(ResourceRef::Application {
                application: "Mail".to_owned()
            })
        );

        let display = tool
            .plan(
                &serde_json::json!({"filename": "shot.png", "target": "display"}),
                &context(),
            )
            .await
            .unwrap();
        assert_eq!(display.capabilities[0].resource, None);
        assert!(display.summary.contains("every other application"));
        // Both still have to be allowed to write where they are writing.
        assert_eq!(display.capabilities[1].domain, "filesystem");
    }

    #[tokio::test]
    async fn showing_a_capture_to_a_model_needs_more_than_saving_one() {
        let desktop: Arc<dyn Desktop> = Arc::new(RecordingDesktop::in_front("Mail"));
        let tool = tool_named(desktop, "computer.screenshot");

        let saved = tool
            .plan(
                &serde_json::json!({
                    "filename": "shot.png", "target": "window", "application": "Mail"
                }),
                &context(),
            )
            .await
            .unwrap();
        assert!(
            !saved
                .capabilities
                .iter()
                .any(|capability| capability.action == VISION_ACTION),
            "an upgrade must not turn an existing screenshot grant into an egress grant"
        );

        let shown = tool
            .plan(
                &serde_json::json!({
                    "filename": "shot.png", "target": "window",
                    "application": "Mail", "attach": true
                }),
                &context(),
            )
            .await
            .unwrap();
        let vision = shown
            .capabilities
            .iter()
            .find(|capability| capability.action == VISION_ACTION)
            .expect("attaching requires the vision capability");
        assert_eq!(
            vision.resource,
            Some(ResourceRef::Application {
                application: "Mail".to_owned()
            }),
            "showing one window to a model is not showing the screen to a model"
        );
    }

    #[tokio::test]
    async fn a_capture_that_is_only_shown_asks_for_no_write() {
        let desktop: Arc<dyn Desktop> = Arc::new(RecordingDesktop::in_front("Mail"));
        let plan = tool_named(desktop, "computer.screenshot")
            .plan(
                &serde_json::json!({"target": "display", "attach": true}),
                &context(),
            )
            .await
            .unwrap();
        assert!(
            !plan
                .capabilities
                .iter()
                .any(|capability| capability.domain == "filesystem"),
            "nothing is written, so nothing should be asked for"
        );
        // A whole display, leaving the machine.
        assert_eq!(plan.risk, RiskLevel::High);
    }

    #[tokio::test]
    async fn a_capture_for_nobody_is_refused() {
        let desktop: Arc<dyn Desktop> = Arc::new(RecordingDesktop::in_front("Mail"));
        let tool = tool_named(desktop, "computer.screenshot");
        assert!(
            tool.validate(&serde_json::json!({"target": "display"}))
                .is_err(),
            "a capture that is neither saved nor shown still reads the screen"
        );
        assert!(
            tool.validate(&serde_json::json!({"target": "display", "attach": true}))
                .is_ok()
        );
    }

    #[tokio::test]
    async fn an_attached_capture_comes_back_as_an_untrusted_image() {
        let desktop: Arc<dyn Desktop> = Arc::new(RecordingDesktop::in_front("Mail"));
        let output = tool_named(desktop, "computer.screenshot")
            .execute(
                serde_json::json!({
                    "target": "window", "application": "Mail", "attach": true
                }),
                &context(),
                CancellationToken::new(),
            )
            .await
            .unwrap();

        assert_eq!(output.images.len(), 1);
        let image = &output.images[0];
        assert_eq!(
            image.source,
            DataSource::Screen {
                target: "Mail".to_owned()
            },
            "pixels carry the provenance the taint tracker reads"
        );
        assert_eq!(image.format, agentos_core::trust::ImageFormat::Png);
        assert!(!image.is_empty());
        assert_eq!(output.structured.as_ref().unwrap()["attached"], true);
        assert!(
            output.structured.as_ref().unwrap()["path"].is_null(),
            "no filename was given, so nothing was written"
        );
    }

    #[tokio::test]
    async fn a_capture_that_is_not_attached_carries_no_pixels() {
        let desktop: Arc<dyn Desktop> = Arc::new(RecordingDesktop::in_front("Mail"));
        let directory = tempfile::tempdir().unwrap();
        let output = tool_named(desktop, "computer.screenshot")
            .execute(
                serde_json::json!({
                    "filename": "shot.png", "target": "window", "application": "Mail"
                }),
                &ToolContext::new(
                    AgentId::new(),
                    TaskId::new(),
                    TaskRunId::new(),
                    directory.path().to_path_buf(),
                ),
                CancellationToken::new(),
            )
            .await
            .unwrap();

        assert!(output.images.is_empty());
        assert!(directory.path().join("shot.png").exists());
    }

    #[tokio::test]
    async fn a_screenshot_says_which_of_the_two_it_is_taking() {
        let desktop: Arc<dyn Desktop> = Arc::new(RecordingDesktop::in_front("Mail"));
        let tool = tool_named(desktop, "computer.screenshot");
        assert!(
            tool.validate(&serde_json::json!({"filename": "a.png", "target": "window"}))
                .is_err(),
            "a window capture must name the window"
        );
        assert!(
            tool.validate(&serde_json::json!({
                "filename": "a.png", "target": "display", "application": "Mail"
            }))
            .is_err(),
            "naming an application would imply a narrowing that is not happening"
        );
    }

    #[tokio::test]
    async fn a_screenshot_cannot_write_outside_the_workspace() {
        let desktop: Arc<dyn Desktop> = Arc::new(RecordingDesktop::in_front("Mail"));
        let plan = tool_named(desktop, "computer.screenshot")
            .plan(
                &serde_json::json!({"filename": "../../escaped.png", "target": "display"}),
                &context(),
            )
            .await
            .unwrap();
        let path = plan.capabilities[1].resource.clone().unwrap();
        // Resolution happens before the policy sees it, so the policy compares a
        // canonical path rather than one containing `..`.
        assert!(!path.to_string().contains(".."));
    }

    #[tokio::test]
    async fn reading_the_desktop_raises_taint_and_moving_the_mouse_does_not() {
        let desktop: Arc<dyn Desktop> = Arc::new(RecordingDesktop::in_front("Mail"));
        for tool in all(desktop) {
            let metadata = tool.metadata();
            let reads = matches!(
                metadata.name.as_str(),
                "computer.inspect" | "computer.screenshot"
            );
            assert_eq!(
                metadata.returns_untrusted_data, reads,
                "`{}` reports the wrong taint disposition",
                metadata.name
            );
        }
    }

    #[tokio::test]
    async fn what_the_screen_reports_is_untrusted_and_says_where_it_came_from() {
        let desktop: Arc<dyn Desktop> = Arc::new(RecordingDesktop::in_front("Mail"));
        let output = tool_named(desktop, "computer.inspect")
            .execute(serde_json::json!({}), &context(), CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            output.content.source,
            DataSource::Screen {
                target: "desktop".to_owned()
            }
        );
        assert!(output.content.source.is_externally_influenced());
    }
}
