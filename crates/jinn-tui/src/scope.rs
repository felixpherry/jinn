//! Keymap scopes for context-sensitive key handling.
//!
//! The scope determines which set of keybindings is active.
//! Each sidebar section has its own scope so section-specific keys
//! (like `r` for rename vs pin-relative) are unambiguous.

/// The current keymap context.
///
/// Controls which keybindings are active. Set via
/// [`ratatui_which_key::WhichKeyState::set_scope`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Scope {
    /// Normal mode - navigation and commands.
    Normal,
    /// Sidebar - Persona section.
    SidebarPersona,
    /// Sidebar - Pins section.
    SidebarPins,
    /// Sidebar - Sessions section.
    SidebarSessions,
    /// Sidebar - Task list section.
    SidebarTaskList,
    /// Sidebar - MCP servers section.
    SidebarMcpServers,
    /// Picker - Provider/model selection.
    PickerProvider,
    /// Picker - Session browser.
    PickerSession,
    /// Picker - Persona selection.
    PickerPersona,
    /// Picker - Theme selection.
    PickerTheme,
    /// Picker - Session lifecycle recipe selection.
    PickerLifecycle,

    /// Picker - Compaction model selection.
    PickerCompactionModel,
    /// Picker - Reasoning effort selection.
    PickerReasoningEffort,
    /// Picker - OpenRouter routing endpoint selection.
    PickerEndpoint,
    /// Picker - Tool toggle selection.
    PickerTool,
    /// Picker - Skill toggle selection.
    PickerSkill,
    /// Picker - Read-only task list browser.
    PickerTaskList,
    /// Picker - Curated project directory selection.
    PickerProject,
    /// Picker - MCP server toggle selection.
    PickerMcpServer,
    /// Picker - Read-only plugin list.
    PickerPlugin,
    /// Picker - Subscription provider to log in to.
    PickerAuthLogin,
    /// Picker - How to complete the chosen provider's login.
    PickerAuthMethod,
    /// Picker - The modal carrying an authorization through to completion.
    PickerAuthProgress,
    /// Picker - Provider to log out of.
    PickerAuthLogout,
    /// Input mode - typing into the input buffer.
    Input,
    /// Arg input mode - typing positional args for a lifecycle command.
    ArgInput,
    /// Token budget input mode - typing a numeric budget value.
    TokenBudgetInput,
    /// Rename session input mode - editing a session title.
    RenameSessionInput,
    /// CWD input mode - typing a directory path.
    CwdInput,
    /// Dashboard tab — service status overview.
    Dashboard,
    /// Terminal tab — viewing an `interactive_term` session (passive).
    TerminalView,
    /// Terminal control — keys forward to the pty; handback key exits.
    TerminalControl,
    /// Project-add input mode - typing a directory path to register a project.
    ProjectAddInput,
    /// Pruner accumulation threshold input mode - numeric input for the KV-cache gate.
    PrunerAccumulationInput,

    /// Quake bar overlay - global console. Captures all keystrokes while open.
    QuakeBar,

    /// Sidebar resize mode - adjusting sidebar width.
    SidebarResize,
}

impl std::fmt::Display for Scope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Normal => write!(f, "Normal"),
            Self::SidebarPersona => write!(f, "SidebarPersona"),
            Self::SidebarPins => write!(f, "SidebarPins"),
            Self::SidebarSessions => write!(f, "SidebarSessions"),
            Self::SidebarTaskList => write!(f, "SidebarTaskList"),
            Self::SidebarMcpServers => write!(f, "SidebarMcpServers"),
            Self::PickerProvider => write!(f, "Picker(provider)"),
            Self::PickerSession => write!(f, "Picker(session)"),
            Self::PickerPersona => write!(f, "Picker(persona)"),
            Self::PickerTheme => write!(f, "Picker(theme)"),
            Self::PickerLifecycle => write!(f, "Picker(lifecycle)"),

            Self::PickerCompactionModel => write!(f, "Picker(compaction-model)"),
            Self::PickerReasoningEffort => write!(f, "Picker(reasoning-effort)"),
            Self::PickerEndpoint => write!(f, "Picker(endpoint)"),
            Self::PickerTool => write!(f, "Picker(tool)"),
            Self::PickerSkill => write!(f, "Picker(skill)"),
            Self::PickerTaskList => write!(f, "Picker(task-list)"),
            Self::PickerProject => write!(f, "Picker(project)"),
            Self::PickerMcpServer => write!(f, "Picker(mcp-server)"),
            Self::PickerPlugin => write!(f, "Picker(plugin)"),
            Self::PickerAuthLogin => write!(f, "Picker(auth-login)"),
            Self::PickerAuthMethod => write!(f, "Picker(auth-method)"),
            Self::PickerAuthProgress => write!(f, "Picker(auth-progress)"),
            Self::PickerAuthLogout => write!(f, "Picker(auth-logout)"),
            Self::Input => write!(f, "Input"),
            Self::Dashboard => write!(f, "Dashboard"),
            Self::TerminalView => write!(f, "TerminalView"),
            Self::TerminalControl => write!(f, "TerminalControl"),
            Self::ArgInput => write!(f, "ArgInput"),
            Self::TokenBudgetInput => write!(f, "TokenBudgetInput"),
            Self::SidebarResize => write!(f, "SidebarResize"),
            Self::RenameSessionInput => write!(f, "RenameSessionInput"),
            Self::CwdInput => write!(f, "CwdInput"),
            Self::ProjectAddInput => write!(f, "ProjectAddInput"),
            Self::PrunerAccumulationInput => write!(f, "PrunerAccumulationInput"),
            Self::QuakeBar => write!(f, "QuakeBar"),
        }
    }
}

impl std::str::FromStr for Scope {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Normal" => Ok(Self::Normal),
            "SidebarPersona" => Ok(Self::SidebarPersona),
            "SidebarPins" => Ok(Self::SidebarPins),
            "SidebarSessions" => Ok(Self::SidebarSessions),
            "SidebarTaskList" => Ok(Self::SidebarTaskList),
            "SidebarMcpServers" => Ok(Self::SidebarMcpServers),
            "Picker(provider)" => Ok(Self::PickerProvider),
            "Picker(session)" => Ok(Self::PickerSession),
            "Picker(persona)" => Ok(Self::PickerPersona),
            "Picker(theme)" => Ok(Self::PickerTheme),
            "Picker(lifecycle)" => Ok(Self::PickerLifecycle),

            "Picker(compaction-model)" => Ok(Self::PickerCompactionModel),
            "Picker(reasoning-effort)" => Ok(Self::PickerReasoningEffort),
            "Picker(endpoint)" => Ok(Self::PickerEndpoint),
            "Picker(tool)" => Ok(Self::PickerTool),
            "Picker(skill)" => Ok(Self::PickerSkill),
            "Picker(task-list)" => Ok(Self::PickerTaskList),
            "Picker(project)" => Ok(Self::PickerProject),
            "Picker(mcp-server)" => Ok(Self::PickerMcpServer),
            "Picker(plugin)" => Ok(Self::PickerPlugin),
            "Picker(auth-login)" => Ok(Self::PickerAuthLogin),
            "Picker(auth-method)" => Ok(Self::PickerAuthMethod),
            "Picker(auth-progress)" => Ok(Self::PickerAuthProgress),
            "Picker(auth-logout)" => Ok(Self::PickerAuthLogout),
            "Input" => Ok(Self::Input),
            "Dashboard" => Ok(Self::Dashboard),
            "ArgInput" => Ok(Self::ArgInput),
            "TokenBudgetInput" => Ok(Self::TokenBudgetInput),
            "RenameSessionInput" => Ok(Self::RenameSessionInput),
            "CwdInput" => Ok(Self::CwdInput),
            "ProjectAddInput" => Ok(Self::ProjectAddInput),
            "PrunerAccumulationInput" => Ok(Self::PrunerAccumulationInput),
            "QuakeBar" => Ok(Self::QuakeBar),
            "SidebarResize" => Ok(Self::SidebarResize),

            _ => Err(()),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, reason = "test code")]

    use super::Scope;
    use std::str::FromStr;

    #[rstest::rstest]
    fn picker_reasoning_effort_scope_round_trips() {
        // Given the PickerReasoningEffort scope variant.
        // When formatting then parsing back.
        // Then the round-trip preserves the variant.
        let s = Scope::PickerReasoningEffort.to_string();
        assert_eq!(s, "Picker(reasoning-effort)");
        assert_eq!(
            Scope::from_str(&s),
            Ok(Scope::PickerReasoningEffort),
            "Display/FromStr should round-trip"
        );
    }

    #[rstest::rstest]
    #[test]
    fn dashboard_scope_round_trips() {
        // Given the Dashboard scope variant.
        // When formatting then parsing back.
        // Then the round-trip preserves the variant.
        let s = Scope::Dashboard.to_string();
        assert_eq!(s, "Dashboard");
        assert_eq!(
            Scope::from_str(&s),
            Ok(Scope::Dashboard),
            "Display/FromStr should round-trip"
        );
    }

    #[rstest::rstest]
    #[test]
    fn plugin_picker_scope_round_trips() {
        // Given the PickerPlugin scope variant.
        // When formatting then parsing back.
        // Then the round-trip preserves the variant.
        let s = Scope::PickerPlugin.to_string();
        assert_eq!(s, "Picker(plugin)");
        assert_eq!(
            Scope::from_str(&s),
            Ok(Scope::PickerPlugin),
            "Display/FromStr should round-trip"
        );
    }
}
