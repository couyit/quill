use std::ops::Mul;

use bevy::{prelude::*, ui};
use bevy_mod_picking::prelude::*;
use bevy_mod_stylebuilder::*;
use bevy_quill_core::{prelude::*, IntoViewChild, ViewChild};
use bevy_quill_obsidian::{
    colors,
    cursor::StyleBuilderCursor,
    hooks::{UseElementRect, UseIsHover},
};

use crate::{DragAction, DragMode, Gesture, GestureState, GraphEvent};

fn style_node_graph_node(ss: &mut StyleBuilder) {
    ss.display(ui::Display::Flex)
        .flex_direction(ui::FlexDirection::Column)
        .align_items(ui::AlignItems::Stretch)
        .position(ui::PositionType::Absolute)
        .visible(false);
}

const NODE_BORDER_RADIUS: f32 = 5.;
const NODE_BORDER_WIDTH: f32 = 1.;

fn style_node_graph_node_title(ss: &mut StyleBuilder) {
    ss.border(1)
        .border_color(colors::U4)
        .border(ui::UiRect {
            left: ui::Val::Px(NODE_BORDER_WIDTH),
            right: ui::Val::Px(NODE_BORDER_WIDTH),
            top: ui::Val::Px(NODE_BORDER_WIDTH),
            bottom: ui::Val::Px(0.),
        })
        .border_radius(ui::BorderRadius {
            top_left: ui::Val::Px(NODE_BORDER_RADIUS),
            top_right: ui::Val::Px(NODE_BORDER_RADIUS),
            bottom_left: ui::Val::Px(0.),
            bottom_right: ui::Val::Px(0.),
        })
        .background_color(colors::Y_GREEN.darker(0.05))
        .padding((6, 2))
        .cursor(SystemCursorIcon::Grab);
}

fn style_node_graph_node_content(ss: &mut StyleBuilder) {
    ss.display(ui::Display::Flex)
        .flex_direction(ui::FlexDirection::Column)
        .align_items(ui::AlignItems::Stretch)
        .gap(4)
        .border(1)
        .border_color(colors::U4)
        .border(ui::UiRect {
            left: ui::Val::Px(NODE_BORDER_WIDTH),
            right: ui::Val::Px(NODE_BORDER_WIDTH),
            top: ui::Val::Px(0.),
            bottom: ui::Val::Px(NODE_BORDER_WIDTH),
        })
        .border_radius(ui::BorderRadius {
            top_left: ui::Val::Px(0.),
            top_right: ui::Val::Px(0.),
            bottom_left: ui::Val::Px(NODE_BORDER_RADIUS),
            bottom_right: ui::Val::Px(NODE_BORDER_RADIUS),
        })
        .background_color(colors::U2)
        .padding((0, 6));
}

fn style_node_graph_node_shadow(ss: &mut StyleBuilder) {
    ss.position(ui::PositionType::Absolute)
        .left(-3)
        .top(-3)
        .right(-3)
        .bottom(-3)
        .border_radius(NODE_BORDER_RADIUS + 3.)
        .background_color(Srgba::new(0., 0., 0., 0.7))
        .pointer_events(false);
}

fn style_node_graph_node_outline(ss: &mut StyleBuilder) {
    ss.position(ui::PositionType::Absolute)
        .left(-3)
        .top(-3)
        .right(-3)
        .bottom(-3)
        .border(2)
        .border_color(colors::FOCUS)
        .border_radius(NODE_BORDER_RADIUS + 3.)
        .pointer_events(false);
}

/// A node within a node graph.
#[derive(Clone, PartialEq)]
pub struct NodeDisplay {
    /// Entity id for the UI element to display.
    pub display_id: Entity,
    /// Entity id of the node.
    pub node_id: Entity,
    /// The coordinates of the node's upper-left corner.
    pub position: IVec2,
    /// Display width of the node.
    pub width: ui::Val,
    /// The title of the node.
    pub title: String,
    /// Whether the node is currently selected.
    pub selected: bool,
    /// The content of the node.
    pub children: ViewChild,
}

impl NodeDisplay {
    /// Create a new node display.
    pub fn new(display_id: Entity, node_id: Entity) -> Self {
        Self {
            display_id,
            node_id,
            position: default(),
            title: default(),
            width: ui::Val::Auto,
            selected: false,
            children: default(),
        }
    }

    /// Set the seletion state of the node.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Set the position of the node.
    pub fn position(mut self, position: IVec2) -> Self {
        self.position = position;
        self
    }

    /// Set the display width of the node.
    pub fn width(mut self, width: ui::Val) -> Self {
        self.width = width;
        self
    }

    /// Set the title of the node.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Set the children of the node.
    pub fn children(mut self, children: impl IntoViewChild) -> Self {
        self.children = children.into_view_child();
        self
    }
}

impl ViewTemplate for NodeDisplay {
    type View = impl View;
    fn create(&self, cx: &mut Cx) -> Self::View {
        let position = self.position;
        let node_id = self.node_id;
        let display_id = self.display_id;
        let hovering = cx.is_hovered(display_id);
        let rect = cx.use_element_rect(display_id);

        Element::<Node>::for_entity(display_id)
            .named("NodeGraph::Node")
            .style(style_node_graph_node)
            .insert_dyn(move |_| node_event_handlers(display_id, node_id), ())
            .effect(
                move |cx, ent, (position, size)| {
                    if size.x > 0 && size.y > 0 {
                        let mut style = cx.world_mut().get_mut::<Style>(ent).unwrap();
                        style.left = ui::Val::Px((position.x - size.x) as f32);
                        style.top = ui::Val::Px((position.y - size.y) as f32);
                        let mut visibility = cx.world_mut().get_mut::<Visibility>(ent).unwrap();
                        *visibility = Visibility::Visible;
                    }
                },
                (position, rect.size().mul(0.5).as_ivec2()),
            )
            .children((
                Element::<Node>::new()
                    .named("NodeGraph::Node::Shadow")
                    .style(style_node_graph_node_shadow),
                Element::<Node>::new()
                    .named("NodeGraph::Node::Title")
                    .style(style_node_graph_node_title)
                    .style_dyn(
                        |selected, sb| {
                            sb.border_color(if selected {
                                colors::FOREGROUND
                            } else {
                                colors::U4
                            });
                        },
                        self.selected,
                    )
                    .insert_dyn(move |_| title_event_handlers(display_id), ())
                    .children(self.title.clone()),
                Element::<Node>::new()
                    .style(style_node_graph_node_content)
                    .style_dyn(
                        |selected, sb| {
                            sb.border_color(if selected {
                                colors::FOREGROUND
                            } else {
                                colors::U4
                            });
                        },
                        self.selected,
                    )
                    .style_dyn(
                        |width, sb| {
                            sb.width(width);
                        },
                        self.width,
                    )
                    .children(self.children.clone()),
                Cond::new(
                    hovering,
                    Element::<Node>::new()
                        .named("NodeGraph::Node::Outline")
                        .style(style_node_graph_node_outline),
                    (),
                ),
            ))
    }
}

#[allow(clippy::type_complexity)]
fn node_event_handlers(id: Entity, node_id: Entity) -> (On<Pointer<Down>>, On<Pointer<DragStart>>) {
    (
        On::<Pointer<Down>>::run(
            move |mut event: ListenerMut<Pointer<Down>>,
                  mut writer: EventWriter<GraphEvent>,
                  keys: Res<ButtonInput<KeyCode>>| {
                event.stop_propagation();
                let is_shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
                let is_ctrl =
                    keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::Meta]);
                if is_ctrl {
                    writer.send(GraphEvent {
                        target: id,
                        gesture: Gesture::SelectToggle(node_id),
                    });
                } else if is_shift {
                    writer.send(GraphEvent {
                        target: id,
                        gesture: Gesture::SelectAdd(node_id),
                    });
                } else {
                    writer.send(GraphEvent {
                        target: id,
                        gesture: Gesture::Select(node_id),
                    });
                }
            },
        ),
        On::<Pointer<DragStart>>::run(move |mut event: ListenerMut<Pointer<DragStart>>| {
            event.stop_propagation();
        }),
    )
}

#[allow(clippy::type_complexity)]
fn title_event_handlers(
    id: Entity,
) -> (
    On<Pointer<DragStart>>,
    On<Pointer<DragEnd>>,
    On<Pointer<Drag>>,
) {
    (
        On::<Pointer<DragStart>>::run(
            move |mut event: ListenerMut<Pointer<DragStart>>,
                  mut gesture_state: ResMut<GestureState>,
                  mut writer: EventWriter<GraphEvent>| {
                event.stop_propagation();
                gesture_state.mode = DragMode::Move;
                writer.send(GraphEvent {
                    target: id,
                    gesture: Gesture::Move(Vec2::default(), DragAction::Start),
                });
            },
        ),
        On::<Pointer<DragEnd>>::run(
            move |mut event: ListenerMut<Pointer<DragEnd>>,
                  mut gesture_state: ResMut<GestureState>,
                  mut writer: EventWriter<GraphEvent>| {
                event.stop_propagation();
                if gesture_state.mode == DragMode::Move {
                    gesture_state.mode = DragMode::None;
                    writer.send(GraphEvent {
                        target: id,
                        gesture: Gesture::Move(event.distance, DragAction::Finish),
                    });
                }
            },
        ),
        On::<Pointer<Drag>>::run({
            move |mut event: ListenerMut<Pointer<Drag>>,
                  gesture_state: ResMut<GestureState>,
                  mut writer: EventWriter<GraphEvent>| {
                event.stop_propagation();
                if gesture_state.mode == DragMode::Move {
                    writer.send(GraphEvent {
                        target: id,
                        gesture: Gesture::Move(event.distance, DragAction::Update),
                    });
                }
            }
        }),
    )
}
