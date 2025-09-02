use bevy::prelude::*;
use bevy_mod_stylebuilder::*;
use bevy_quill_core::*;

fn style_spacer(ss: &mut StyleBuilder) {
    ss.flex_grow(1.);
}

/// A spacer widget that fills the available space.
#[derive(Clone, PartialEq, Default)]
pub struct Spacer;

impl ViewTemplate for Spacer {
    type View = impl View;

    fn create(&self, _cx: &mut Cx) -> Self::View {
        Element::<Node>::new().style(style_spacer)
    }
}
