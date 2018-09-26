use ::Page;

/// Get a block from a page
/// Block borrows page so only one block at a time
/// Maybe use a base "Ptr" struct and have traits for access?
struct Block {
    base: Page,
}
