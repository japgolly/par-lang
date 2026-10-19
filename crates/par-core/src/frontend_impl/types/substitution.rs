use super::super::language::{LocalName, TypeParameter};
use super::core::Type;
use super::error::TypeError;
use crate::frontend_impl::types::visit;
use crate::location::Span;
use std::collections::BTreeMap;

impl<S: Clone> Type<S> {
    pub fn substitute(self, map: BTreeMap<&LocalName, &Type<S>>) -> Result<Self, TypeError<S>> {
        fn contains_free_self<S>(typ: &Type<S>, target: &Option<LocalName>) -> bool {
            match typ {
                Type::Self_(_, label) | Type::DualSelf(_, label) => label == target,
                Type::Recursive { label, .. } | Type::Iterative { label, .. }
                    if label == target =>
                {
                    false
                }
                _ => {
                    let mut found = false;
                    visit::continue_(typ, |child| {
                        found |= contains_free_self(child, target);
                        Ok::<_, ()>(())
                    })
                    .unwrap();
                    found
                }
            }
        }

        fn contains_self_label<S>(typ: &Type<S>, target: &Option<LocalName>) -> bool {
            if match typ {
                Type::Self_(_, label)
                | Type::DualSelf(_, label)
                | Type::Recursive { label, .. }
                | Type::Iterative { label, .. } => label == target,
                _ => false,
            } {
                return true;
            }

            let mut found = false;
            visit::continue_(typ, |child| {
                found |= contains_self_label(child, target);
                Ok::<_, ()>(())
            })
            .unwrap();
            found
        }

        fn fresh_self_label<S>(
            old_label: &Option<LocalName>,
            body: &Type<S>,
            map: &BTreeMap<&LocalName, &Type<S>>,
        ) -> LocalName {
            let mut candidate = LocalName {
                span: old_label
                    .as_ref()
                    .map_or(Span::None, |label| label.span.clone()),
                string: match old_label {
                    Some(label) => arcstr::format!("{}'", label.string),
                    None => arcstr::literal!("self'"),
                },
            };
            while {
                let label = Some(candidate.clone());
                contains_self_label(body, &label)
                    || map.values().any(|typ| contains_self_label(typ, &label))
            } {
                candidate.string = arcstr::format!("{}'", candidate.string);
            }
            candidate
        }

        fn rename_bound_self<S>(
            typ: &mut Type<S>,
            old_label: &Option<LocalName>,
            new_label: &LocalName,
        ) {
            match typ {
                Type::Self_(_, label) | Type::DualSelf(_, label) if label == old_label => {
                    *label = Some(new_label.clone());
                }
                Type::Recursive { label, .. } | Type::Iterative { label, .. }
                    if label == old_label =>
                {
                    // A nested fixpoint shadows the binder being renamed.
                }
                _ => {
                    visit::continue_mut(typ, |child| {
                        rename_bound_self(child, old_label, new_label);
                        Ok::<_, ()>(())
                    })
                    .unwrap();
                }
            }
        }

        fn inner<S: Clone>(
            typ: &mut Type<S>,
            map: &BTreeMap<&LocalName, &Type<S>>,
        ) -> Result<(), TypeError<S>> {
            match typ {
                Type::Var(_span, name) if map.contains_key(name) => {
                    *typ = map.get(name).cloned().cloned().unwrap();
                }
                Type::DualVar(_span, name) if map.contains_key(name) => {
                    *typ = map.get(name).cloned().cloned().unwrap().dual(Span::None);
                }
                Type::Exists(_span, param, body) | Type::Forall(_span, param, body) => {
                    let map = enter_binders(std::slice::from_mut(param), &mut [&mut **body], map)?;
                    inner(body, &map)?
                }
                Type::Pair(_span, left, right, params)
                | Type::Function(_span, left, right, params)
                    if !params.is_empty() =>
                {
                    let map = enter_binders(params, &mut [&mut **left, &mut **right], map)?;
                    inner(left, &map)?;
                    inner(right, &map)?;
                }
                Type::Recursive {
                    label,
                    body,
                    display_hint,
                    ..
                }
                | Type::Iterative {
                    label,
                    body,
                    display_hint,
                    ..
                } => {
                    let old_label = label.clone();
                    if map
                        .values()
                        .any(|replacement| contains_free_self(replacement, &old_label))
                    {
                        let new_label = fresh_self_label(&old_label, body, map);
                        rename_bound_self(body, &old_label, &new_label);
                        *label = Some(new_label);
                    }
                    inner(body, map)?;
                    if let Some(display_hint) = display_hint.0.as_mut() {
                        for arg in &mut display_hint.args {
                            inner(arg, map)?;
                        }
                    }
                }
                _ => {
                    visit::continue_mut(typ, |child: &mut Type<S>| inner(child, map))?;
                }
            }
            Ok(())
        }

        /// Removes the names bound by `params` from `map`, first renaming those binders that
        /// would capture a free variable of a replacement.
        fn enter_binders<'m, S: Clone>(
            params: &mut [TypeParameter],
            bodies: &mut [&mut Type<S>],
            map: &BTreeMap<&'m LocalName, &'m Type<S>>,
        ) -> Result<BTreeMap<&'m LocalName, &'m Type<S>>, TypeError<S>> {
            let mut map = map.clone();
            for param in params.iter() {
                map.remove(&param.name);
            }
            let renames = rename_capturing_binders(params, bodies, |name| {
                map.values().any(|typ| typ.contains_var(name))
            });
            if !renames.is_empty() {
                let renames = renames.iter().map(|(old, new)| (old, new)).collect();
                for body in bodies.iter_mut() {
                    inner(body, &renames)?;
                }
            }
            Ok(map)
        }

        let mut typ = self;
        inner(&mut typ, &map)?;
        Ok(typ)
    }

    pub fn contains_var(&self, var: &LocalName) -> bool {
        fn inner<S>(result: &mut bool, typ: &Type<S>, target_name: &LocalName) -> Result<(), ()> {
            match typ {
                Type::Var(_span, name) | Type::DualVar(_span, name) if name == target_name => {
                    *result = true;
                }
                Type::Forall(_, param, _) | Type::Exists(_, param, _)
                    if &param.name == target_name =>
                {
                    // var is shadowed
                }
                Type::Pair(_, _, _, params) | Type::Function(_, _, _, params)
                    if params.iter().any(|param| &param.name == target_name) =>
                {
                    // var is shadowed
                }
                _ => {
                    visit::continue_(typ, |child| inner(result, child, target_name))?;
                }
            }
            Ok(())
        }
        let mut result = false;
        inner(&mut result, self, var).unwrap();
        result
    }

    pub fn substitute_inferred_holes(self, map: &BTreeMap<LocalName, Type<S>>) -> Self {
        fn inner<S: Clone>(typ: &mut Type<S>, map: &BTreeMap<LocalName, Type<S>>) {
            match typ {
                Type::Hole(_span, name, _) => {
                    if let Some(replacement) = map.get(name) {
                        *typ = replacement.clone();
                    }
                }
                Type::DualHole(_span, name, _) => {
                    if let Some(replacement) = map.get(name) {
                        *typ = replacement.clone().dual(Span::None);
                    }
                }
                Type::Exists(_span, param, body) | Type::Forall(_span, param, body) => {
                    enter_binders(std::slice::from_mut(param), &mut [&mut **body], map);
                    inner(body, map);
                }
                Type::Pair(_span, left, right, params)
                | Type::Function(_span, left, right, params)
                    if !params.is_empty() =>
                {
                    enter_binders(params, &mut [&mut **left, &mut **right], map);
                    inner(left, map);
                    inner(right, map);
                }
                Type::Recursive {
                    body, display_hint, ..
                }
                | Type::Iterative {
                    body, display_hint, ..
                } => {
                    inner(body, map);
                    if let Some(display_hint) = display_hint.0.as_mut() {
                        for arg in &mut display_hint.args {
                            inner(arg, map);
                        }
                    }
                }
                _ => {
                    visit::continue_mut(typ, |child| {
                        inner(child, map);
                        Ok::<_, ()>(())
                    })
                    .unwrap();
                }
            }
        }

        /// Renames binders that would capture a free variable of a hole's solution.
        fn enter_binders<S: Clone>(
            params: &mut [TypeParameter],
            bodies: &mut [&mut Type<S>],
            map: &BTreeMap<LocalName, Type<S>>,
        ) {
            let renames = rename_capturing_binders(params, bodies, |name| {
                map.values().any(|typ| typ.contains_var(name))
            });
            if !renames.is_empty() {
                let renames: BTreeMap<_, _> = renames.iter().map(|(old, new)| (old, new)).collect();
                for body in bodies.iter_mut() {
                    **body = body
                        .clone()
                        .substitute(renames.clone())
                        .unwrap_or_else(|_| unreachable!("renaming type variables cannot fail"));
                }
            }
        }

        let mut typ = self;
        inner(&mut typ, map);
        typ
    }
}

/// Renames those of `params` for which `captures` holds to fresh names, free in neither
/// the `bodies` nor the replacements, and distinct from the other `params`. Returns the
/// renames, which the caller must apply to the `bodies`.
fn rename_capturing_binders<S: Clone>(
    params: &mut [TypeParameter],
    bodies: &[&mut Type<S>],
    captures: impl Fn(&LocalName) -> bool,
) -> Vec<(LocalName, Type<S>)> {
    let mut renames = Vec::new();
    for i in 0..params.len() {
        let old_name = params[i].name.clone();
        if !captures(&old_name) {
            continue;
        }
        let mut new_name = old_name.clone();
        while captures(&new_name)
            || params.iter().any(|param| param.name == new_name)
            || bodies.iter().any(|body| body.contains_var(&new_name))
        {
            new_name.string = arcstr::format!("{}'", new_name.string);
        }
        params[i].name = new_name.clone();
        renames.push((old_name, Type::Var(new_name.span.clone(), new_name)));
    }
    renames
}
