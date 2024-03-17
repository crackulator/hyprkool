use std::path::PathBuf;

use anyhow::{anyhow, Context};
use clap::{arg, command, Parser, Subcommand};
use hyprland::{
    data::{CursorPosition, Monitor, Workspace},
    dispatch::{Dispatch, DispatchType, WorkspaceIdentifierWithSpecial},
    event_listener::EventListener,
    shared::{HyprData, HyprDataActive, WorkspaceType},
};
use serde::{Deserialize, Serialize};

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Cli {
    /// Specify a custom config directory
    #[arg(short, long)]
    pub config_dir: Option<String>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Deserialize, Debug)]
pub struct Config {
    // pub workspace_names: Vec<String>,
    pub activities: Vec<String>,
    pub enable_animations: bool,
    pub animation_duration: u64,
    pub workspace_switch_animation_curve: Option<String>,
    pub workspace_horizontal_switch_animation_style: Option<String>,
    pub workspace_vertical_switch_animation_style: Option<String>,
    pub acitvity_switch_animation_curve: Option<String>,
    pub acitvity_switch_animation_style: Option<String>,

    /// mouse polling rate in ms
    pub polling_rate: u64,
    /// number of pixels to consider as edge
    pub edge_width: u64,
    /// push cursor inside margin when it loops
    pub edge_margin: u64,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            activities: vec!["default".into()],
            enable_animations: true,
            animation_duration: 6,
            workspace_switch_animation_curve: None,
            workspace_horizontal_switch_animation_style: None,
            workspace_vertical_switch_animation_style: None,
            acitvity_switch_animation_curve: None,
            acitvity_switch_animation_style: None,
            polling_rate: 300,
            edge_width: 0,
            edge_margin: 2,
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum Command {
    MouseLoop,
    PrintActivityStatus,
    MoveRight {
        #[arg(long, short, default_value_t = false)]
        cycle: bool,

        /// move focused window and move to workspace
        #[arg(long, short = 'w', default_value_t = false)]
        move_window: bool,
    },
    MoveLeft {
        #[arg(long, short, default_value_t = false)]
        cycle: bool,

        /// move focused window and move to workspace
        #[arg(long, short = 'w', default_value_t = false)]
        move_window: bool,
    },
    MoveUp {
        #[arg(long, short, default_value_t = false)]
        cycle: bool,

        /// move focused window and move to workspace
        #[arg(long, short = 'w', default_value_t = false)]
        move_window: bool,
    },
    MoveDown {
        #[arg(long, short, default_value_t = false)]
        cycle: bool,

        /// move focused window and move to workspace
        #[arg(long, short = 'w', default_value_t = false)]
        move_window: bool,
    },
    NextActivity {
        #[arg(long, short, default_value_t = false)]
        cycle: bool,

        /// move focused window and move to workspace
        #[arg(long, short = 'w', default_value_t = false)]
        move_window: bool,
    },
    PrevActivity {
        #[arg(long, short, default_value_t = false)]
        cycle: bool,

        /// move focused window and move to workspace
        #[arg(long, short = 'w', default_value_t = false)]
        move_window: bool,
    },
    SwitchToActivity {
        /// <activity name>
        #[arg(short, long)]
        name: String,

        /// move focused window and move to workspace
        #[arg(long, short = 'w', default_value_t = false)]
        move_window: bool,
    },
    SwitchToWorkspaceInActivity {
        /// <workspace name>
        #[arg(short, long)]
        name: String,

        /// move focused window and move to workspace
        #[arg(long, short = 'w', default_value_t = false)]
        move_window: bool,
    },
    SwitchToWorkspace {
        /// <activity name>:<workspace name>
        #[arg(short, long)]
        name: String,

        /// move focused window and move to workspace
        #[arg(long, short = 'w', default_value_t = false)]
        move_window: bool,
    },
}

#[derive(Debug)]
pub struct State {
    pub activities: Vec<String>,
    pub workspaces: Vec<Vec<String>>,
    pub config: Config,
}

impl State {
    fn new(config: Config) -> Self {
        let raw_workspaces = [1, 2, 3, 4, 5, 6, 7, 8, 9];
        let mut activities = config.activities.clone();
        if activities.is_empty() {
            activities.push("default".into());
        }
        let cooked_workspaces = activities
            .iter()
            .map(|name| {
                raw_workspaces
                    .iter()
                    .cloned()
                    .map(|id| format!("{name}:{id}"))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        Self {
            activities,
            workspaces: cooked_workspaces,
            config,
        }
    }

    fn get_activity_index(&self, name: impl AsRef<str>) -> Option<usize> {
        let name = name.as_ref();
        let activity_index = self.activities.iter().position(|a| name.starts_with(a))?;
        Some(activity_index)
    }

    /// (activity index, workspace index)
    fn get_indices(&self, name: impl AsRef<str>) -> Option<(usize, Option<usize>)> {
        let name = name.as_ref();
        let activity_index = self.get_activity_index(name)?;
        let workspace_index = self.workspaces[activity_index]
            .iter()
            .position(|w| w == name);
        Some((activity_index, workspace_index))
    }

    async fn moved_workspace(&self, x: i64, y: i64, cycle: bool) -> anyhow::Result<&str> {
        let workspace = Workspace::get_active_async().await?;
        let Some((activity_index, Some(workspace_index))) = self.get_indices(workspace.name) else {
            return Err(anyhow!("Error: not in a valid activity workspace"));
        };
        let mut iy = workspace_index as i64 / 3;
        let mut ix = workspace_index as i64 % 3;
        if cycle {
            ix += x + 3;
            ix %= 3;
            iy += y + 3;
            iy %= 3;
        } else {
            ix += x;
            ix = ix.max(0).min(2);
            iy += y;
            iy = iy.max(0).min(2);
        }
        Ok(&self.workspaces[activity_index][iy as usize * 3 + ix as usize])
    }

    async fn move_to_workspace(&self, name: impl AsRef<str>, move_window: bool) -> anyhow::Result<()> {
        let name = name.as_ref();
        if move_window {
            Dispatch::call_async(DispatchType::MoveToWorkspace(
                WorkspaceIdentifierWithSpecial::Name(name),
                None,
            ))
            .await?;
        } else {
            Dispatch::call_async(DispatchType::Workspace(
                WorkspaceIdentifierWithSpecial::Name(name),
            ))
            .await?;
        }
        Ok(())
    }

    fn get_activity_status_repr(&self, workspace_name: &str) -> Option<String> {
        let Some((activity_index, Some(workspace_index))) = self.get_indices(workspace_name) else {
            return None;
        };

        let mut activity = String::new();
        for (i, _) in self.workspaces[activity_index].iter().enumerate() {
            if i == 0 {
            } else if i % 3 == 0 && i > 0 && i < 9 {
                activity += "\n";
            } else {
                activity += " ";
            }
            if i == workspace_index {
                activity += "   ";
            } else {
                activity += "███";
            }
        }

        Some(activity)
    }

    async fn set_animation_vertical(&self) -> anyhow::Result<()> {
        let Some(curve) = self.config.workspace_switch_animation_curve.as_deref() else {
            return Ok(());
        };
        let mut command = tokio::process::Command::new("hyprctl");
        command.args([
            "keyword",
            "animation",
            &format!(
                "workspaces,{},{},{},{}",
                if self.config.enable_animations { 1 } else { 0 },
                self.config.animation_duration,
                curve,
                self.config
                    .workspace_vertical_switch_animation_style
                    .as_deref()
                    .unwrap_or(""),
            ),
        ]);
        let _ = command.output().await?;
        Ok(())
    }

    async fn set_animation_horizontal(&self) -> anyhow::Result<()> {
        let Some(curve) = self.config.workspace_switch_animation_curve.as_deref() else {
            return Ok(());
        };
        let mut command = tokio::process::Command::new("hyprctl");
        command.args([
            "keyword",
            "animation",
            &format!(
                "workspaces,{},{},{},{}",
                if self.config.enable_animations { 1 } else { 0 },
                self.config.animation_duration,
                curve,
                self.config
                    .workspace_horizontal_switch_animation_style
                    .as_deref()
                    .unwrap_or(""),
            ),
        ]);
        let _ = command.output().await?;
        Ok(())
    }

    async fn set_activity_animation(&self) -> anyhow::Result<()> {
        let Some(curve) = self.config.acitvity_switch_animation_curve.as_deref() else {
            return Ok(());
        };
        let mut command = tokio::process::Command::new("hyprctl");
        command.args([
            "keyword",
            "animation",
            &format!(
                "workspaces,{},{},{},{}",
                if self.config.enable_animations { 1 } else { 0 },
                self.config.animation_duration,
                curve,
                self.config
                    .acitvity_switch_animation_style
                    .as_deref()
                    .unwrap_or(""),
            ),
        ]);
        let _ = command.output().await?;
        Ok(())
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = cli
        .config_dir
        .clone()
        .map(PathBuf::from)
        .or(dirs::config_dir().map(|pb| pb.join("hypr/hyprkool.toml")))
        .map(std::fs::read_to_string)
        .transpose()?
        .map(|s| toml::from_str::<Config>(&s))
        .transpose()?
        .unwrap_or(Config::default());
    let state = State::new(config);

    match cli.command {
        Command::MouseLoop => {
            Dispatch::call_async(DispatchType::Workspace(
                WorkspaceIdentifierWithSpecial::Name(&state.workspaces[0][0]),
            ))
            .await?;

            // TODO: multi monitor setup yaaaaaaaaaaaaaaaaa
            let monitor = Monitor::get_active_async().await?;
            let w = state.config.edge_width as i64;
            let m = state.config.edge_margin as i64;

            loop {
                tokio::time::sleep(std::time::Duration::from_millis(state.config.polling_rate))
                    .await;
                let mut c = CursorPosition::get_async().await?;
                let mut y = 0;
                let mut x = 0;
                if c.x <= w {
                    x += 3 - 1;
                    c.x = monitor.width as i64 - m;
                } else if c.x >= monitor.width as i64 - 1 - w {
                    x += 1;
                    c.x = 2;
                }
                if c.y <= w {
                    y += 3 - 1;
                    c.y = monitor.height as i64 - m;
                } else if c.y >= monitor.height as i64 - 1 - w {
                    y += 1;
                    c.y = 2;
                }

                if x + y == 0 {
                    continue;
                }

                let workspace = Workspace::get_active_async().await?;
                let Some((current_activity_index, Some(current_workspace_index))) =
                    state.get_indices(&workspace.name)
                else {
                    println!("unknown workspace {}", workspace.name);
                    continue;
                };

                let move_vert = y > 0;
                let move_hor = x > 0;
                y += current_workspace_index / 3;
                y %= 3;
                x += current_workspace_index % 3;
                x %= 3;

                let new_workspace = &state.workspaces[current_activity_index][y * 3 + x];
                if new_workspace != &workspace.name {
                    if move_hor {
                        state.set_animation_horizontal().await?;
                    } else if move_vert {
                        state.set_animation_vertical().await?;
                    }
                    state.move_to_workspace(new_workspace, false).await?;
                    Dispatch::call_async(DispatchType::MoveCursor(c.x, c.y)).await?;
                }
            }
        }
        Command::SwitchToWorkspace { name, move_window } => {
            let (activity_index, workspace_index) =
                state.get_indices(&name).context("activity not found")?;
            let workspace_index = workspace_index.context("workspace not found")?;
            let new_workspace = &state.workspaces[activity_index][workspace_index];
            state.set_activity_animation().await?;
            state.move_to_workspace(new_workspace, move_window).await?;
        }
        Command::SwitchToWorkspaceInActivity { name, move_window } => {
            let workspace = Workspace::get_active_async().await?;
            let activity_index = state
                .get_activity_index(&workspace.name)
                .context("could not get current activity")?;
            let activity = &state.activities[activity_index];
            let new_workspace = format!("{activity}:{name}");
            state.set_activity_animation().await?;
            state.move_to_workspace(&new_workspace, move_window).await?;
        }
        Command::SwitchToActivity {
            mut name,
            move_window,
        } => {
            let workspace = Workspace::get_active_async().await?;
            if let Some(activity_index) = state.get_activity_index(&workspace.name) {
                let activity = &state.activities[activity_index];
                let id = workspace
                    .name
                    .strip_prefix(activity)
                    .expect("just checked this");
                name.push_str(id);
            } else {
                name.push('0');
            };
            state.set_activity_animation().await?;
            state.move_to_workspace(&name, move_window).await?;
        }
        Command::NextActivity { cycle, move_window } => {
            let workspace = Workspace::get_active_async().await?;
            let activity_index = state.get_activity_index(&workspace.name);
            let new_activity_index = activity_index
                .map(|i| {
                    let mut i = i;
                    if cycle {
                        i += 1;
                        i %= state.activities.len();
                    } else {
                        i = i.min(state.activities.len() - 1);
                    }
                    i
                })
                .unwrap_or(0);
            let id = activity_index.and_then(|i| workspace.name.strip_prefix(&state.activities[i]));
            let mut name = state.activities[new_activity_index].clone();
            if let Some(id) = id {
                name.push_str(id);
            } else {
                name = state.workspaces[new_activity_index][0].clone();
            };
            state.set_activity_animation().await?;
            state.move_to_workspace(&name, move_window).await?;
        }
        Command::PrevActivity { cycle, move_window } => {
            let workspace = Workspace::get_active_async().await?;
            let activity_index = state.get_activity_index(&workspace.name);
            let new_activity_index = activity_index
                .map(|i| {
                    let mut i = i as isize;
                    if cycle {
                        i += state.activities.len() as isize - 1;
                        i %= state.activities.len() as isize;
                    } else {
                        i = i.max(0);
                    }
                    i as usize
                })
                .unwrap_or(0);
            let id = activity_index.and_then(|i| workspace.name.strip_prefix(&state.activities[i]));
            let activity_index = new_activity_index;
            let mut name = state.activities[activity_index].clone();
            if let Some(id) = id {
                name.push_str(id);
            } else {
                name = state.workspaces[activity_index][0].clone();
            };
            state.set_activity_animation().await?;
            state.move_to_workspace(&name, move_window).await?;
        }
        Command::MoveRight { cycle, move_window } => {
            let workspace = state.moved_workspace(1, 0, cycle).await?;
            state.set_animation_horizontal().await?;
            state.move_to_workspace(workspace, move_window).await?;
        }
        Command::MoveLeft { cycle, move_window } => {
            let workspace = state.moved_workspace(-1, 0, cycle).await?;
            state.set_animation_horizontal().await?;
            state.move_to_workspace(workspace, move_window).await?;
        }
        Command::MoveUp { cycle, move_window } => {
            let workspace = state.moved_workspace(0, -1, cycle).await?;
            state.set_animation_vertical().await?;
            state.move_to_workspace(workspace, move_window).await?;
        }
        Command::MoveDown { cycle, move_window } => {
            let workspace = state.moved_workspace(0, 1, cycle).await?;
            state.set_animation_vertical().await?;
            state.move_to_workspace(workspace, move_window).await?;
        }
        Command::PrintActivityStatus => {
            #[derive(Deserialize, Serialize, Debug)]
            struct ActivityStatus {
                text: String,
            }
            fn print_state(state: &State, name: &str) {
                state
                    .get_activity_status_repr(name)
                    .into_iter()
                    .for_each(|a| {
                        println!(
                            "{}",
                            serde_json::to_string(&ActivityStatus { text: a })
                                .expect("it will work")
                        );
                    });
            }

            let workspace = Workspace::get_active_async().await?;
            print_state(&state, &workspace.name);

            let mut ael = EventListener::new();
            ael.add_workspace_change_handler(move |e| match e {
                WorkspaceType::Regular(name) => {
                    print_state(&state, &name);
                }
                WorkspaceType::Special(..) => {}
            });
            ael.start_listener_async().await?;
        }
    }

    Ok(())
}
