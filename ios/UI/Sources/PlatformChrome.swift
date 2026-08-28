import SwiftUI

/// The handful of modifiers that exist on one platform and not the other.
///
/// Here so that a screen shared between the phone and the Mac can be one file rather
/// than two. The alternative was a `#if os(iOS)` inside every such view, and the
/// alternative to *that* was a second copy of the view for macOS -- which is how
/// `ItemsModel` and `ListsModel` came to be needed in the first place. A shared screen
/// that differs only in its chrome should differ only here.
extension View {

    /// A title that sits on the same line as the bar, where there is a bar to sit on.
    ///
    /// macOS has no navigation bar and no title display mode; a window's title is the
    /// window's. Nothing is lost by doing nothing there.
    func compactTitle() -> some View {
        #if os(iOS) || os(watchOS)
            return navigationBarTitleDisplayMode(.inline)
        #else
            return self
        #endif
    }

    /// A field that expects an email address: the right keyboard, and no capital
    /// letter forced onto the front of it.
    ///
    /// Both are about a software keyboard, so both are about a phone. A Mac has a real
    /// keyboard and no opinion to correct.
    func emailEntry() -> some View {
        #if os(iOS)
            return keyboardType(.emailAddress)
                .textInputAutocapitalization(.never)
        #else
            return self
        #endif
    }
}

extension ToolbarItemPlacement {

    /// The leading end of a sheet's toolbar.
    ///
    /// `.topBarLeading` does not exist on macOS and `.navigation` is what it is called
    /// there. Both put the item at the leading end, which is the whole requirement:
    /// adding on the left and finishing on the right is the convention these sheets
    /// follow -- see `TagsView`.
    static var sheetLeading: ToolbarItemPlacement {
        #if os(iOS) || os(watchOS)
            .topBarLeading
        #else
            .navigation
        #endif
    }
}
