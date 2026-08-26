import SwiftUI

/// The tags on a row, degrading in two steps rather than being squeezed.
///
/// A Mac window is resizable, so a row that fits at one width does not fit at another,
/// and the thing that gives way should be the least useful part. In order:
///
/// 1. **Names and emoji**, while there is room for them.
/// 2. **Emoji alone.** The name is the part a glance does not need — the list is
///    already ordered by these, so the mark is a reminder rather than a label.
/// 3. **As many emoji as fit, then `…`.** Not a smaller emoji, not a wrapped second
///    line, and not a run of glyphs clipped mid-character: an ellipsis says "there are
///    more" in the space one of them would have taken, which is the honest answer at
///    that width.
///
/// It never wraps and never truncates a name mid-word. A row is one line.
struct MacTagStrip: View {
    let tags: [Tag]

    var body: some View {
        ViewThatFits(in: .horizontal) {
            // Everything, while it fits.
            HStack(spacing: 4) {
                ForEach(tags) { chip($0) }
            }

            // Emoji alone, dropping what will not fit. This one always "fits" —
            // `EllipsisRow` reports the width it was given rather than the width it
            // wanted — so it is the last resort and the layout never falls through it.
            EllipsisRow(spacing: 4) {
                ForEach(tags) { mark($0) }
                Text("…")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .accessibilityElement()
        .accessibilityLabel(
            tags.isEmpty ? "" : "In " + tags.map(\.name).joined(separator: ", ")
        )
    }

    /// A tag with its name: quiet, and not a control.
    ///
    /// Nothing here is tappable. Changing what an item is filed under is the editor's
    /// job, and a chip that sometimes removes a tag when you meant to cross the item
    /// off is the reason the phone keeps them in the sheet too.
    private func chip(_ tag: Tag) -> some View {
        Text(tag.emoji.flatMap { $0.isEmpty ? nil : "\($0) \(tag.name)" } ?? tag.name)
            .font(.caption)
            .foregroundStyle(.secondary)
            .fixedSize()
            .padding(.horizontal, 6)
            .padding(.vertical, 1)
            .background(.quaternary, in: Capsule())
            .accessibilityHidden(true)
            .accessibilityIdentifier("chip.\(tag.name)")
    }

    /// The same tag in one glyph, with no capsule: at this width the capsule is
    /// decoration competing with the thing it decorates.
    private func mark(_ tag: Tag) -> some View {
        Text(tag.mark)
            .font(.caption)
            .fixedSize()
            .accessibilityHidden(true)
    }
}

/// A row that drops what will not fit and says so with an ellipsis.
///
/// The last subview is the ellipsis; everything before it is content. It is placed only
/// when something has been dropped, and collapsed to nothing when everything fitted —
/// so the caller writes the ellipsis unconditionally and this decides whether it is
/// true.
struct EllipsisRow: Layout {
    var spacing: CGFloat = 4

    /// Reports the width it was offered, not the width it wanted.
    ///
    /// That is what makes it a safe last resort inside `ViewThatFits`: a layout that
    /// asked for its natural width would be rejected at exactly the widths it exists to
    /// handle, and the whole strip would fall back to nothing.
    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let content = subviews.dropLast()
        let natural = width(of: content)
        let height = subviews.map { $0.sizeThatFits(.unspecified).height }.max() ?? 0
        return CGSize(width: min(natural, proposal.width ?? natural), height: height)
    }

    func placeSubviews(
        in bounds: CGRect,
        proposal: ProposedViewSize,
        subviews: Subviews,
        cache: inout ()
    ) {
        guard let ellipsis = subviews.last else { return }
        let content = Array(subviews.dropLast())

        // How many fit as they are. Counted from the left, because that is the order
        // the list is walked in — the tag that placed the row comes first, and it is
        // the one worth keeping when only one survives.
        let fitting = howManyFit(content, within: bounds.width, room: 0)

        if fitting == content.count {
            place(content, from: bounds, count: content.count)
            collapse(ellipsis, at: bounds)
            return
        }

        // Something is being dropped, so the ellipsis has to be paid for out of the
        // same width -- otherwise it is what overflows.
        let ellipsisWidth = ellipsis.sizeThatFits(.unspecified).width + spacing
        let shown = howManyFit(content, within: bounds.width, room: ellipsisWidth)

        let after = place(content, from: bounds, count: shown)
        ellipsis.place(
            at: CGPoint(x: after, y: bounds.midY),
            anchor: .leading,
            proposal: .unspecified
        )
        for extra in content.dropFirst(shown) { collapse(extra, at: bounds) }
    }

    private func width(of subviews: some Collection<LayoutSubview>) -> CGFloat {
        let widths = subviews.map { $0.sizeThatFits(.unspecified).width }
        return widths.reduce(0, +) + spacing * CGFloat(max(0, widths.count - 1))
    }

    private func howManyFit(
        _ content: [LayoutSubview],
        within available: CGFloat,
        room reserved: CGFloat
    ) -> Int {
        var used: CGFloat = 0
        var count = 0
        for subview in content {
            let next = used + (count == 0 ? 0 : spacing) + subview.sizeThatFits(.unspecified).width
            if next + reserved > available { break }
            used = next
            count += 1
        }
        return count
    }

    /// Places the first `count` subviews and answers where the next one would start.
    @discardableResult
    private func place(_ content: [LayoutSubview], from bounds: CGRect, count: Int) -> CGFloat {
        var x = bounds.minX
        for subview in content.prefix(count) {
            subview.place(
                at: CGPoint(x: x, y: bounds.midY),
                anchor: .leading,
                proposal: .unspecified
            )
            x += subview.sizeThatFits(.unspecified).width + spacing
        }
        return x
    }

    /// Out of the way and out of the drawing, for a subview this width has no room for.
    private func collapse(_ subview: LayoutSubview, at bounds: CGRect) {
        subview.place(
            at: CGPoint(x: bounds.minX, y: bounds.midY),
            anchor: .leading,
            proposal: ProposedViewSize(width: 0, height: 0)
        )
    }
}
