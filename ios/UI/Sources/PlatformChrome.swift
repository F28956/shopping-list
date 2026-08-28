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

    /// A sheet big enough to show what is in it.
    ///
    /// iOS presents a sheet at a size the system chooses and the content fills it. A
    /// Mac sizes the sheet to its content, and a `List` inside a `NavigationStack` has
    /// no intrinsic height to offer -- so it reports zero, and the sheet comes up as a
    /// title and a footer with a scroll view of height 0 between them.
    ///
    /// That is not a subtle degradation. `TagsView` shipped on the Mac with all
    /// twenty-one categories present in the view, laid out at 24pt each, inside a
    /// scroll area measured at 470x0. The screen was simply empty.
    func sheetSize() -> some View {
        #if os(macOS)
            return frame(minWidth: 420, idealWidth: 460, minHeight: 360, idealHeight: 520)
        #else
            return self
        #endif
    }

    /// The two buttons a sheet is driven by: one that adds, one that finishes.
    ///
    /// On iOS they go in the navigation bar, adding on the left and finishing on the
    /// right, as Settings > Passwords does. **On macOS a `.toolbar` inside a
    /// `NavigationStack` inside a sheet renders nothing at all** -- which is how the
    /// Categories sheet shipped on the Mac with no way to add a category and no way to
    /// close it except by guessing that Return would. So there they become a button row
    /// along the bottom, which is where a Mac sheet puts them anyway.
    ///
    /// One definition rather than a `#if` in each screen, for the reason the rest of
    /// this file exists: a screen shared between the two should differ in one place.
    func sheetActions<Adding: View, Finishing: View>(
        @ViewBuilder adding: () -> Adding,
        @ViewBuilder finishing: () -> Finishing
    ) -> some View {
        #if os(macOS)
            return VStack(spacing: 0) {
                self
                Divider()
                HStack {
                    adding()
                    Spacer()
                    finishing()
                }
                .padding(.horizontal, 20)
                .padding(.vertical, 12)
            }
        #else
            return toolbar {
                ToolbarItem(placement: .topBarLeading) { adding() }
                ToolbarItem(placement: .confirmationAction) { finishing() }
            }
        #endif
    }

    /// The same, for a sheet that is cancelled rather than added to.
    func sheetActions<Cancelling: View, Confirming: View>(
        cancelling: () -> Cancelling,
        confirming: () -> Confirming
    ) -> some View {
        #if os(macOS)
            return VStack(spacing: 0) {
                self
                Divider()
                HStack {
                    Spacer()
                    cancelling()
                    confirming()
                }
                .padding(.horizontal, 20)
                .padding(.vertical, 12)
            }
        #else
            return toolbar {
                ToolbarItem(placement: .cancellationAction) { cancelling() }
                ToolbarItem(placement: .confirmationAction) { confirming() }
            }
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
