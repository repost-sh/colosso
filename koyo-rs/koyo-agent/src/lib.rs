//! The app's side of the agent host (`docs/chat.md` 5.1): the protocol it speaks, as serde
//! types, the client that speaks it, and an open thread's rows as they were last told. Nothing
//! here draws, so all of it is tested without compiling the app.

pub mod agent_client;
pub mod client;
pub mod protocol;
pub mod replica;
pub mod timeline;
pub mod words;
