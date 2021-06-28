use crate::{
    builtins::iterable::get_iterator,
    environment::{
        declarative_environment_record::DeclarativeEnvironmentRecord,
        lexical_environment::VariableScope,
    },
    exec::{Executable, InterpreterState},
    gc::{Finalize, Trace},
    syntax::ast::node::{Node, NodeKind},
    BoaProfiler, Context, Result, Value,
};
use std::fmt;

#[cfg(feature = "deser")]
use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "deser", derive(Serialize, Deserialize))]
#[derive(Clone, Debug, Trace, Finalize, PartialEq)]
pub struct ForOfLoop {
    variable: Box<Node>,
    iterable: Box<Node>,
    body: Box<Node>,
    label: Option<Box<str>>,
}

impl ForOfLoop {
    pub fn new<V, I, B>(variable: V, iterable: I, body: B) -> Self
    where
        V: Into<Node>,
        I: Into<Node>,
        B: Into<Node>,
    {
        Self {
            variable: Box::new(variable.into()),
            iterable: Box::new(iterable.into()),
            body: Box::new(body.into()),
            label: None,
        }
    }

    pub fn variable(&self) -> &Node {
        &self.variable
    }

    pub fn iterable(&self) -> &Node {
        &self.iterable
    }

    pub fn body(&self) -> &Node {
        &self.body
    }

    pub fn label(&self) -> Option<&str> {
        self.label.as_ref().map(Box::as_ref)
    }

    pub fn set_label(&mut self, label: Box<str>) {
        self.label = Some(label);
    }

    pub fn display(&self, f: &mut fmt::Formatter<'_>, indentation: usize) -> fmt::Result {
        if let Some(ref label) = self.label {
            write!(f, "{}: ", label)?;
        }
        write!(f, "for ({} of {}) ", self.variable, self.iterable)?;
        self.body().display(f, indentation)
    }
}

impl fmt::Display for ForOfLoop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.display(f, 0)
    }
}

impl From<ForOfLoop> for NodeKind {
    fn from(for_of: ForOfLoop) -> Self {
        Self::ForOfLoop(for_of)
    }
}

impl Executable for ForOfLoop {
    fn run(&self, context: &mut Context) -> Result<Value> {
        let _timer = BoaProfiler::global().start_event("ForOf", "exec");
        let iterable = self.iterable().run(context)?;
        let iterator = get_iterator(context, iterable)?;
        let mut result = Value::undefined();

        loop {
            {
                let env = context.get_current_environment();
                context.push_environment(DeclarativeEnvironmentRecord::new(Some(env)));
            }
            let iterator_result = iterator.next(context)?;
            if iterator_result.is_done() {
                context.pop_environment();
                break;
            }
            let next_result = iterator_result.value();

            match self.variable().kind() {
                NodeKind::Identifier(ref name) => {
                    if context.has_binding(name.as_ref()) {
                        // Binding already exists
                        context.set_mutable_binding(name.as_ref(), next_result.clone(), true)?;
                    } else {
                        context.create_mutable_binding(
                            name.as_ref().to_owned(),
                            true,
                            VariableScope::Function,
                        )?;
                        context.initialize_binding(name.as_ref(), next_result.clone())?;
                    }
                }
                NodeKind::VarDeclList(ref list) => match list.as_ref() {
                    [var] => {
                        if var.init().is_some() {
                            return context.throw_syntax_error("a declaration in the head of a for-of loop can't have an initializer");
                        }

                        if context.has_binding(var.name()) {
                            context.set_mutable_binding(var.name(), next_result, true)?;
                        } else {
                            context.create_mutable_binding(
                                var.name().to_owned(),
                                false,
                                VariableScope::Function,
                            )?;
                            context.initialize_binding(var.name(), next_result)?;
                        }
                    }
                    _ => {
                        return context.throw_syntax_error(
                            "only one variable can be declared in the head of a for-of loop",
                        )
                    }
                },
                NodeKind::LetDeclList(ref list) => match list.as_ref() {
                    [var] => {
                        if var.init().is_some() {
                            return context.throw_syntax_error("a declaration in the head of a for-of loop can't have an initializer");
                        }

                        context.create_mutable_binding(
                            var.name().to_owned(),
                            false,
                            VariableScope::Block,
                        )?;

                        context.initialize_binding(var.name(), next_result)?;
                    }
                    _ => {
                        return context.throw_syntax_error(
                            "only one variable can be declared in the head of a for-of loop",
                        )
                    }
                },
                NodeKind::ConstDeclList(ref list) => match list.as_ref() {
                    [var] => {
                        if var.init().is_some() {
                            return context.throw_syntax_error("a declaration in the head of a for-of loop can't have an initializer");
                        }

                        context.create_immutable_binding(
                            var.name().to_owned(),
                            false,
                            VariableScope::Block,
                        )?;
                        context.initialize_binding(var.name(), next_result)?;
                    }
                    _ => {
                        return context.throw_syntax_error(
                            "only one variable can be declared in the head of a for-of loop",
                        )
                    }
                },
                NodeKind::Assign(_) => {
                    return context.throw_syntax_error(
                        "a declaration in the head of a for-of loop can't have an initializer",
                    );
                }
                _ => {
                    return context
                        .throw_syntax_error("unknown left hand side in head of for-of loop")
                }
            }

            result = self.body().run(context)?;
            match context.executor().get_current_state() {
                InterpreterState::Break(label) => {
                    handle_state_with_labels!(self, label, context, break);
                    break;
                }
                InterpreterState::Continue(label) => {
                    handle_state_with_labels!(self, label, context, continue);
                }
                InterpreterState::Return => return Ok(result),
                InterpreterState::Executing => {
                    // Continue execution.
                }
                #[cfg(feature = "vm")]
                InterpreterState::Error => {}
            }
            let _ = context.pop_environment();
        }
        Ok(result)
    }
}
