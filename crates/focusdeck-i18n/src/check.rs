use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use fluent_syntax::ast::{
    CallArguments, Entry, Expression, InlineExpression, Message, Pattern, PatternElement,
};
use fluent_syntax::parser;

type Variables = BTreeMap<String, BTreeSet<String>>;

pub fn check_locales(dir: &Path) -> Result<(), String> {
    let mut files = ftl_files(dir)?;
    files.sort();

    let mut problems = Vec::new();
    let mut english = None;
    let mut others = Vec::new();
    for name in &files {
        let text = fs::read_to_string(dir.join(name)).map_err(|e| format!("{name}: {e}"))?;
        let (resource, errors) = match parser::parse(text.as_str()) {
            Ok(resource) => (resource, Vec::new()),
            Err((resource, errors)) => (resource, errors),
        };
        for error in errors {
            let start = error.slice.as_ref().map_or(error.pos.start, |s| s.start);
            let line = line_of(&text, start);
            problems.push(format!("{name}:{line}: syntax error: {}", error.kind));
        }
        let variables = message_variables(&resource.body);
        if name == "en.ftl" {
            english = Some(variables);
        } else {
            others.push((name, variables));
        }
    }

    let Some(english) = english else {
        problems.push("en.ftl: file not found".to_owned());
        return Err(problems.join("\n"));
    };

    for (name, variables) in &others {
        for (id, expected) in &english {
            match variables.get(id) {
                None => problems.push(format!("{name}: missing message `{id}`")),
                Some(found) if found != expected => problems.push(format!(
                    "{name}: message `{id}` variables {} differ from en.ftl {}",
                    render(found),
                    render(expected)
                )),
                Some(_) => {}
            }
        }
        for id in variables.keys().filter(|id| !english.contains_key(*id)) {
            problems.push(format!("{name}: extra message `{id}`"));
        }
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("\n"))
    }
}

fn ftl_files(dir: &Path) -> Result<Vec<String>, String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut files = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| format!("{}: {e}", dir.display()))?.path();
        if path.extension().is_some_and(|ext| ext == "ftl")
            && let Some(name) = path.file_name().and_then(|n| n.to_str())
        {
            files.push(name.to_owned());
        }
    }
    Ok(files)
}

fn render(vars: &BTreeSet<String>) -> String {
    let joined: Vec<&str> = vars.iter().map(String::as_str).collect();
    format!("{{{}}}", joined.join(", "))
}

fn message_variables(body: &[Entry<&str>]) -> Variables {
    let mut out = Variables::new();
    for entry in body {
        if let Entry::Message(message) = entry {
            out.insert(message.id.name.to_owned(), variables_of(message));
        }
    }
    out
}

fn variables_of(message: &Message<&str>) -> BTreeSet<String> {
    let mut vars = BTreeSet::new();
    if let Some(pattern) = &message.value {
        pattern_vars(pattern, &mut vars);
    }
    for attribute in &message.attributes {
        pattern_vars(&attribute.value, &mut vars);
    }
    vars
}

fn pattern_vars(pattern: &Pattern<&str>, vars: &mut BTreeSet<String>) {
    for element in &pattern.elements {
        if let PatternElement::Placeable { expression } = element {
            expression_vars(expression, vars);
        }
    }
}

fn expression_vars(expression: &Expression<&str>, vars: &mut BTreeSet<String>) {
    match expression {
        Expression::Inline(inline) => inline_vars(inline, vars),
        Expression::Select { selector, variants } => {
            inline_vars(selector, vars);
            for variant in variants {
                pattern_vars(&variant.value, vars);
            }
        }
    }
}

fn inline_vars(inline: &InlineExpression<&str>, vars: &mut BTreeSet<String>) {
    match inline {
        InlineExpression::VariableReference { id } => {
            vars.insert(id.name.to_owned());
        }
        InlineExpression::FunctionReference { arguments, .. } => call_vars(arguments, vars),
        InlineExpression::TermReference {
            arguments: Some(arguments),
            ..
        } => call_vars(arguments, vars),
        InlineExpression::Placeable { expression } => expression_vars(expression, vars),
        _ => {}
    }
}

fn call_vars(arguments: &CallArguments<&str>, vars: &mut BTreeSet<String>) {
    for positional in &arguments.positional {
        inline_vars(positional, vars);
    }
    for named in &arguments.named {
        inline_vars(&named.value, vars);
    }
}

// parser slices begin right after the previous entry, so skip leading blank space
fn line_of(text: &str, offset: usize) -> usize {
    let rest = &text[offset.min(text.len())..];
    let first_content = offset.min(text.len()) + rest.len() - rest.trim_start().len();
    text[..first_content].matches('\n').count() + 1
}
