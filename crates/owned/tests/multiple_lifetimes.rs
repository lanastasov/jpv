#[owned::owned]
struct MultipleLifetimes<'a, 'b> {
    #[owned(ty = String)]
    a: &'a str,
    #[owned(ty = String)]
    b: &'b str,
}
