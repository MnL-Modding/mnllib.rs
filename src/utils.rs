#[inline]
pub fn none_if_empty<I, T: AsRef<[I]>>(value: T) -> Option<T> {
    if value.as_ref().is_empty() {
        None
    } else {
        Some(value)
    }
}
#[inline]
pub fn empty_if_none<T>(value: Option<Vec<T>>) -> Vec<T> {
    value.unwrap_or_default()
}

#[inline]
pub fn necessary_padding_for(number: usize, alignment: usize) -> usize {
    (alignment - number % alignment) % alignment
}

pub trait AlignToElements {
    fn align_to_elements(&mut self, alignment: usize);
}

impl<T: Default + Clone> AlignToElements for Vec<T> {
    #[inline]
    fn align_to_elements(&mut self, alignment: usize) {
        self.extend(vec![
            T::default();
            necessary_padding_for(self.len(), alignment)
        ]);
    }
}

#[inline]
pub fn u32_or_max_to_option(value: u32) -> Option<u32> {
    if value == u32::MAX {
        None
    } else {
        Some(value)
    }
}
#[inline]
pub fn u32_or_max_to_option_try_into<T: TryFrom<u32>>(value: u32) -> Result<Option<T>, T::Error> {
    u32_or_max_to_option(value)
        .map(|x| x.try_into())
        .transpose()
}
#[inline]
pub fn option_to_u32_or_max(value: Option<u32>) -> u32 {
    value.unwrap_or(u32::MAX)
}
#[inline]
pub fn option_to_u32_or_max_try_into<T: TryInto<u32>>(value: Option<T>) -> Result<u32, T::Error> {
    Ok(option_to_u32_or_max(
        value.map(|x| x.try_into()).transpose()?,
    ))
}
