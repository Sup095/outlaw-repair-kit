//! Turning a runbook entry into candidate actions.
//!
//! This lives here, rather than in whichever front-end happens to need it,
//! because the command line and the desktop window must plan a fix the same
//! way. If they each built their own candidate list, "what will happen when I
//! press this" would depend on which surface you pressed it from, and the
//! answer to that question is the whole product.

use ork_ai::runbook::RunbookLibrary;

use crate::action::FixAction;
use crate::store::TriageItem;

/// Candidate fixes for one queued problem, least disruptive first.
///
/// Runbook fixes arrive as prose plus, sometimes, a suggested command. Those
/// become [`FixAction::Manual`] -- described for a person to carry out -- and
/// are never executed. Running a command string from a text file automatically
/// would defeat the entire typed-action model, so the tool does not do it.
///
/// Only a `[fixes.action]` recipe naming something in the closed set becomes a
/// real action, and a recipe naming anything else is a fault in the runbook,
/// not a licence to do something approximate: it is logged and demoted back to
/// advice.
pub fn candidates_for(
    item: &TriageItem,
    library: &RunbookLibrary,
    platform: &str,
) -> Vec<FixAction> {
    let Some(entry) = library.lookup(&item.finding) else {
        return Vec::new();
    };

    entry
        .fixes_for(platform)
        .into_iter()
        .map(|fix| {
            if let Some(recipe) = &fix.action {
                match FixAction::from_recipe_for(&recipe.kind, &recipe.target, &item.finding) {
                    Ok(action) => return action,
                    Err(refusal) => {
                        tracing::warn!(
                            entry = entry.id,
                            kind = recipe.kind,
                            %refusal,
                            "runbook recipe refused"
                        );
                    }
                }
            }

            let instruction = match &fix.command {
                Some(command) => format!("{}\n    suggested command: {command}", fix.description),
                None => fix.description.clone(),
            };
            FixAction::Manual { instruction }
        })
        .collect()
}
