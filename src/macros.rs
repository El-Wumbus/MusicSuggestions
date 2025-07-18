use std::fmt;

pub struct RenderNode<F>(pub F);
impl<F> fmt::Display for RenderNode<F>
where
    F: Fn(&mut fmt::Formatter<'_>) -> fmt::Result,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (self.0)(f)
    }
}

#[macro_export]
macro_rules! node {
    ($kind:ident $(, $attr:ident = $val:expr )* => $($child:expr),+ $(,)?) => {
        $crate::macros::RenderNode(|f: &mut std::fmt::Formatter<'_>| {
            write!(f, "<{}", stringify!($kind))?;
            $(write!(f, r#" {}="{}""#, stringify!($attr), $val)?;)*
            write!(f, ">")?;
            $(write!(f, "{}", $child)?;)+
            write!(f, "</{}>", stringify!($kind))
        })
    };

    ($kind:ident $(, $attr:ident = $val:expr )* $(,)?) => {
        $crate::macros::RenderNode(|f: &mut std::fmt::Formatter<'_>| {
            write!(f, "<{}", stringify!($kind))?;
            $(write!(f, r#" {}="{}""#, stringify!($attr), $val)?;)*
            write!(f, " />")
        })
    };
}

#[macro_export]
macro_rules! group_nodes {
    ($lnode:expr $(, $rnode:expr )+) => {
        $crate::macros::RenderNode(|f: &mut std::fmt::Formatter<'_>| {
            write!(f, "{}", $lnode)?;
            $(write!(f, "{}", $rnode)?;)+
            Ok(())
        })
    }
}
