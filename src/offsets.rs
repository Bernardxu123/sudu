use crate::app::Message;
use crate::doc_widget::DocWidgetState;
use iced::advanced::widget::operation::Outcome;
use iced::advanced::widget::Operation;
use iced::widget::scrollable::AbsoluteOffset;
use iced::widget::{operation, Id};
use iced::{Rectangle, Task};
use std::any::Any;
use std::collections::HashMap;

pub const BODY_SCROLL: &str = "sudu-body";
pub const DOC_WIDGET: &str = "sudu-doc";

pub fn body_id() -> Id {
    Id::new(BODY_SCROLL)
}

pub fn doc_id() -> Id {
    Id::new(DOC_WIDGET)
}

pub struct Collect {
    headings: Vec<usize>,
    target_block: Option<usize>,
    out_offsets: HashMap<usize, f32>,
    target_y: Option<f32>,
    visible_block: Option<usize>,
}

impl Collect {
    pub fn new(headings: Vec<usize>, target_block: Option<usize>) -> Self {
        Self {
            headings,
            target_block,
            out_offsets: HashMap::new(),
            target_y: None,
            visible_block: None,
        }
    }
}

impl Operation<Message> for Collect {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<Message>)) {
        operate(self);
    }

    fn custom(&mut self, id: Option<&Id>, _bounds: Rectangle, state: &mut dyn Any) {
        if id != Some(&doc_id()) {
            return;
        }
        if let Some(doc_state) = state.downcast_mut::<DocWidgetState>() {
            let (offsets, target_y) = doc_state.read_offsets(&self.headings, self.target_block);
            self.out_offsets = offsets;
            self.target_y = target_y;
            self.visible_block = Some(doc_state.first_visible_block());
        }
    }

    fn finish(&self) -> Outcome<Message> {
        Outcome::Some(Message::Offsets {
            map: self.out_offsets.clone(),
            target_y: self.target_y,
            visible_block: self.visible_block,
        })
    }
}

pub fn collect(headings: Vec<usize>, target_block: Option<usize>) -> Task<Message> {
    iced::advanced::widget::operate(Collect::new(headings, target_block))
}

pub fn scroll_to_y(y: f32) -> Task<Message> {
    operation::scroll_to(
        body_id(),
        AbsoluteOffset {
            x: 0.0,
            y: y.max(0.0),
        },
    )
}

pub fn snap_to_ratio(y: f32) -> Task<Message> {
    operation::snap_to(
        body_id(),
        iced::widget::scrollable::RelativeOffset {
            x: 0.0,
            y: y.clamp(0.0, 1.0),
        },
    )
}

