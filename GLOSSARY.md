# Herdr Sessionizer

Language for choosing project directories, navigating Herdr workspaces and agents, and returning to recently visited workspaces.

## Language

### Workspaces and agents

**Workspace**:
A Herdr task context containing tabs and panes. A workspace is distinct from the project directory associated with it.
_Avoid_: Session, when referring to a workspace

**User workspace**:
A workspace representing a user's task rather than internal shell-pool infrastructure.

**Open workspace**:
A workspace that currently exists in Herdr, whether or not it is focused or contains an agent.
_Avoid_: Active workspace, when meaning any open workspace

**Focused workspace**:
The workspace that currently has Herdr's focus.
_Avoid_: Active workspace, when meaning the focused workspace

**Tab**:
A subdivision of a workspace containing one or more panes.

**Pane**:
A terminal location within a tab. A pane may contain a shell, an agent, or another program.

**Agent**:
An individual coding-agent instance reported by Herdr, associated with a workspace, tab, and pane when that location is available.
_Avoid_: Agent type, when referring to an individual instance

**Agent type**:
The kind of coding agent, such as Pi or Codex, rather than the identity of an individual instance.

**Agent display name**:
A human-readable name for an individual agent, distinct from its type or terminal location.

**Agent status**:
The state Herdr reports for an agent. The recognized labels are `working`, `done`, `idle`, `blocked`, and `unknown`; an unexpected label remains a reported status rather than a new Sessionizer interpretation.
_Avoid_: Running, as a replacement label for `working`

**Agent snapshot**:
The agent information captured for one picker invocation, including its name, type, status, and location. It is distinct from the agent's potentially changed live state.

**Shell pool**:
Internal infrastructure supplying prepared shells for new terminal work.

**Pool workspace**:
The reserved workspace containing shell-pool infrastructure, not a user task.

### Directory choices

**Project directory**:
A filesystem directory representing a project or task the user wants to open in a workspace.

**Workspace directory**:
The project directory associated with a workspace for picker navigation. It is not necessarily the working directory of every pane in that workspace.

**Directory candidate**:
A path offered as a project-directory choice in the picker.
_Avoid_: Allowable directory, which can imply an access-control restriction

**Unopened directory**:
A directory candidate not associated with an open user workspace under Sessionizer's workspace-matching rules.

**Discovery root**:
A directory beneath which Sessionizer looks for directory candidates.

**Discovery depth**:
The maximum number of directory levels below a discovery root included in discovery. Immediate children are at depth one; the root itself is not a child.

**Explicit inclusion**:
A path deliberately added to the directory choices independently of discovery.

**Exclusion**:
A directory name or exact path omitted from discovery together with its descendants. A name matches at any depth beneath a discovery root; an exclusion does not cancel an explicit inclusion.

**Omitted workspace**:
An open user workspace absent from the picker because no usable workspace directory could be identified.

### Picker and navigation

**Picker**:
The interactive interface for choosing an open workspace, an agent within a workspace, or an unopened directory.

**Workspace row**:
A selectable picker entry representing an open user workspace.

**Agent row**:
A selectable picker entry representing an individual agent, nested beneath its workspace row.

**Display order**:
The order of the rows currently presented by the picker, including the effects of the current query.

**Selection**:
The picker entry the user accepts, distinct from a row that is merely highlighted.

**Visit history**:
The recency order of visited user workspaces that remain open.

**Previous workspace**:
The most recently visited surviving user workspace before the latest visited workspace in visit history.

**Jump-back**:
Navigation to the previous workspace, allowing repeated invocations to alternate between recent workspaces.

**Agent-selection error**:
A failure to navigate to a selected agent's captured location. It is distinct from a configuration or discovery error that prevents normal picker choices from being offered.

**Error-only view**:
A picker view presenting a configuration or discovery failure without selectable navigation targets.

**Error dismissal**:
Acknowledgement that closes an error-only view. It does not mean the failed operation succeeded.
