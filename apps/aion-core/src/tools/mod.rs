pub mod calendar;
pub mod discord;
pub mod home_assistant;
pub mod mcp_consumer;
pub mod sandbox;
pub mod skillbook_tool;

pub use calendar::{CalendarCreateTool, CalendarListTool};
pub use discord::DiscordTool;
pub use home_assistant::HomeAssistantTool;
// McpConsumerTool: stub arquitectural — se activa cuando se configura el primer servidor MCP
pub use sandbox::CodeSandboxTool;
pub use skillbook_tool::SkillBookTool;
