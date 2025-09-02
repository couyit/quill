use crate::BorderColorParam;

use super::builder::StyleBuilder;

#[allow(missing_docs)]
pub trait StyleBuilderBorderColor {
    fn border_color(&mut self, color: impl BorderColorParam) -> &mut Self;
}

impl<'a, 'w> StyleBuilderBorderColor for StyleBuilder<'a, 'w> {
    fn border_color(&mut self, color: impl BorderColorParam) -> &mut Self {
        self.target.insert(color.to_border_color());
        self
    }
}
