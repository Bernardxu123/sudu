use crate::app::Message;
use iced::advanced::widget::operation::{Outcome, Scrollable};
use iced::advanced::widget::Operation;
use iced::widget::scrollable::AbsoluteOffset;
use iced::widget::{operation, Id};
use iced::{Rectangle, Task, Vector};
use std::collections::HashMap;

pub const BODY_SCROLL: &str = "sudu-body";

pub fn body_id() -> Id {
    Id::new(BODY_SCROLL)
}

pub struct Collect {
    targets: HashMap<Id, usize>,
    content_top: Option<f32>,
    out: HashMap<usize, f32>,
}

impl Collect {
    pub fn new(targets: HashMap<Id, usize>) -> Self {
        Self {
            targets,
            content_top: None,
            out: HashMap::new(),
        }
    }
}

impl Operation<Message> for Collect {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<Message>)) {
        operate(self);
    }

    fn scrollable(
        &mut self,
        id: Option<&Id>,
        _bounds: Rectangle,
        content_bounds: Rectangle,
        _translation: Vector,
        _state: &mut dyn Scrollable,
    ) {
        if id == Some(&body_id()) {
            self.content_top = Some(content_bounds.y);
        }
    }

    fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
        let Some(top) = self.content_top else {
            return;
        };
        let Some(id) = id else {
            return;
        };
        if let Some(&index) = self.targets.get(id) {
            self.out.insert(index, (bounds.y - top).max(0.0));
        }
    }

    fn finish(&self) -> Outcome<Message> {
        if self.out.is_empty() {
            Outcome::None
        } else {
            Outcome::Some(Message::Offsets(self.out.clone()))
        }
    }
}

pub fn collect(targets: HashMap<Id, usize>) -> Task<Message> {
    iced::advanced::widget::operate(Collect::new(targets))
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
