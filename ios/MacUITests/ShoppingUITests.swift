import XCTest

/// What the Mac app does, driven the way a person drives it.
///
/// Against a fixed in-memory world rather than a server — see `StubURLProtocol` for
/// why that is a URLProtocol and not a fake `API`: everything above the wire is the
/// real thing, including the decoding that is most likely to break.
final class ShoppingUITests: XCTestCase {
    override func setUp() {
        continueAfterFailure = false
    }

    private func launch(_ scenario: String = "default") -> XCUIApplication {
        let app = XCUIApplication()
        app.launchArguments = ["-uiTesting"]
        app.launchEnvironment["UI_SCENARIO"] = scenario
        app.launch()
        return app
    }

    /// A short wait, because every one of these is waiting on the same thing: a
    /// request that has already been answered in memory.
    private func expect(_ element: XCUIElement, _ message: String = "") {
        XCTAssertTrue(element.waitForExistence(timeout: 5), message)
    }

    // MARK: - What the list shows

    func testTheListShowsWhatIsOnIt() {
        let app = launch()

        expect(app.buttons["item.Milk"], "the list did not load")
        XCTAssertTrue(app.buttons["item.Apples"].exists)
        XCTAssertTrue(app.buttons["item.Bread"].exists)
        XCTAssertTrue(app.buttons["item.Batteries"].exists)
    }

    /// Ordered by the first tag, not alphabetically and not by id. The fixture is
    /// built so those three orders differ: by id it is Milk, Apples, Bread; by name
    /// Apples, Batteries, Bread; by shop order Apples, Bread, Milk.
    func testItemsAreOrderedByCategory() {
        let app = launch()
        expect(app.buttons["item.Apples"])

        let names = ["item.Apples", "item.Bread", "item.Milk", "item.Batteries"]
        let tops = names.map { app.buttons[$0].frame.minY }

        XCTAssertEqual(
            tops, tops.sorted(),
            "expected fruits, then bakery, then dairy, then the untagged one last"
        )
    }

    /// Ordering by category, without headings for it. The tag rides on the row.
    ///
    /// Read from the row's accessibility label rather than from the chips: the chips
    /// are deliberately hidden from VoiceOver, because read separately they arrive as
    /// loose words after the item with nothing to say what they are. The label is the
    /// supported way to ask what a row says, so it is the way this asks.
    func testCategoriesAreNotHeadings() {
        let app = launch()
        expect(app.buttons["item.Apples"])

        XCTAssertTrue(
            app.buttons["item.Apples"].label.contains("fruits"),
            "the row does not say where it lives: \(app.buttons["item.Apples"].label)"
        )
        XCTAssertTrue(app.buttons["item.Milk"].label.contains("dairy"))
        XCTAssertFalse(
            app.staticTexts["fruits"].exists,
            "a category heading came back; tags belong on the line"
        )
    }

    // MARK: - Crossing off

    func testTheCheckboxCrossesAnItemOff() {
        let app = launch()
        let box = app.checkBoxes["cross.Milk"]
        expect(box)

        XCTAssertEqual(box.value as? Int, 0, "Milk starts outstanding")
        box.click()

        // It moves into the done section, which is the visible half of "crossed off".
        expect(app.descendants(matching: .any)["clear.done"], "no done section after crossing one off")
        XCTAssertEqual(app.checkBoxes["cross.Milk"].value as? Int, 1)
    }

    func testTheRowOpensTheEditorRatherThanCrossingOff() {
        let app = launch()
        let row = app.buttons["item.Milk"]
        expect(row)

        row.click()

        expect(app.staticTexts["editor.title"], "clicking the row did not open the editor")
        XCTAssertEqual(
            app.checkBoxes["cross.Milk"].value as? Int, 0,
            "opening the editor also crossed the item off"
        )
    }

    // MARK: - Editing

    func testEditingANameAndAmount() {
        let app = launch()
        expect(app.buttons["item.Apples"])
        app.buttons["item.Apples"].click()

        let name = app.textFields["editor.name"]
        expect(name)
        XCTAssertEqual(name.value as? String, "Apples", "the editor opened on the wrong item")

        name.click()
        name.typeKey("a", modifierFlags: .command)
        name.typeText("Braeburn apples")
        app.buttons["editor.save"].click()

        expect(app.buttons["item.Braeburn apples"], "the edit did not take")
        XCTAssertFalse(app.buttons["item.Apples"].exists)
    }

    func testEditingTheTagsOnAnItem() {
        let app = launch()
        expect(app.buttons["item.Batteries"])
        XCTAssertFalse(app.buttons["item.Batteries"].label.contains("bakery"))

        app.buttons["item.Batteries"].click()
        expect(app.checkBoxes["editor.tag.bakery"])
        app.checkBoxes["editor.tag.bakery"].click()
        app.buttons["editor.save"].click()

        expect(app.staticTexts["editor.title"].exists ? app.buttons["item.Batteries"] : app.buttons["item.Batteries"])
        let said = app.buttons["item.Batteries"].label
        XCTAssertTrue(said.contains("bakery"), "the tag was not attached: \(said)")
    }

    /// Cancel puts back everything, tags included — they are held in the draft rather
    /// than applied as they are ticked, so that this is true of them too.
    func testCancellingKeepsTheTagsAsTheyWere() {
        let app = launch()
        expect(app.buttons["item.Batteries"])

        app.buttons["item.Batteries"].click()
        expect(app.checkBoxes["editor.tag.bakery"])
        app.checkBoxes["editor.tag.bakery"].click()
        app.buttons["editor.cancel"].click()

        expect(app.buttons["item.Batteries"])
        XCTAssertFalse(
            app.buttons["item.Batteries"].label.contains("bakery"),
            "cancel left a tag behind, so tags are being applied as they are ticked"
        )
    }

    // MARK: - Adding

    func testAddingAnItem() {
        let app = launch()
        let field = app.textFields["add.field"]
        expect(field)

        XCTAssertFalse(app.buttons["add.button"].isEnabled, "Add offers to send nothing")

        field.click()
        field.typeText("2 kg carrots")
        XCTAssertTrue(app.buttons["add.button"].isEnabled)
        app.buttons["add.button"].click()

        expect(app.buttons["item.Carrots"], "the item was not added, or not capitalised")
    }

    // MARK: - Managing lists

    func testMakingAList() {
        let app = launch()
        expect(app.buttons["list.new"])

        app.buttons["list.new"].click()
        expect(app.textFields["listname.field"])
        XCTAssertFalse(
            app.buttons["listname.confirm"].isEnabled,
            "a list with no name was offered as creatable"
        )

        app.textFields["listname.field"].typeText("Hardware")
        app.buttons["listname.confirm"].click()

        expect(app.staticTexts["list.Hardware"], "the list was not made")
    }

    /// Made, and then looked at: choosing a list is what making one means.
    func testANewListIsSelected() {
        let app = launch()
        expect(app.buttons["list.new"])

        app.buttons["list.new"].click()
        expect(app.textFields["listname.field"])
        app.textFields["listname.field"].typeText("Hardware")
        app.buttons["listname.confirm"].click()

        expect(app.textFields["add.field"], "the new list was not opened")
        XCTAssertFalse(
            app.buttons["item.Milk"].exists,
            "still showing the list that was open before"
        )
    }

    func testRenamingAList() {
        let app = launch()
        let row = app.staticTexts["list.Home"]
        expect(row)

        row.rightClick()
        expect(app.menuItems["Rename…"])
        app.menuItems["Rename…"].click()

        let field = app.textFields["listname.field"]
        expect(field)
        XCTAssertEqual(field.value as? String, "Home", "the field did not start from the name")

        field.click()
        field.typeKey("a", modifierFlags: .command)
        field.typeText("Weekly shop")
        app.buttons["listname.confirm"].click()

        expect(app.staticTexts["list.Weekly shop"], "the rename did not take")
        XCTAssertFalse(app.staticTexts["list.Home"].exists)
    }

    /// Asked before it goes, because everything on it goes too.
    func testDeletingAListAsks() {
        let app = launch()
        expect(app.staticTexts["list.Home"])

        app.staticTexts["list.Home"].rightClick()
        expect(app.menuItems["Delete…"])
        app.menuItems["Delete…"].click()

        // Scoped to the dialog: "Cancel" and "Delete" are common enough words that
        // a bare query finds several.
        expect(app.buttons["delete.cancel"], "deleting a list did not ask first")
        app.buttons["delete.cancel"].click()

        expect(app.staticTexts["list.Home"], "cancel deleted it anyway")
    }

    func testDeletingAList() {
        let app = launch()
        expect(app.staticTexts["list.Home"])

        app.staticTexts["list.Home"].rightClick()
        expect(app.menuItems["Delete…"])
        app.menuItems["Delete…"].click()
        expect(app.buttons["delete.confirm"])
        app.buttons["delete.confirm"].click()

        let gone = NSPredicate(format: "exists == false")
        expectation(for: gone, evaluatedWith: app.staticTexts["list.Home"])
        waitForExpectations(timeout: 5)
    }

    /// An editor was given a list, not the say over whether it exists.
    func testAViewerIsNotOfferedRenameOrDelete() {
        let app = launch("viewer")
        expect(app.staticTexts["list.Home"])

        app.staticTexts["list.Home"].rightClick()

        XCTAssertFalse(
            app.menuItems["Delete…"].waitForExistence(timeout: 1),
            "a viewer was offered Delete"
        )
        XCTAssertFalse(app.menuItems["Rename…"].exists, "a viewer was offered Rename")
    }

    // MARK: - What a viewer is not offered

    /// A viewer is given a list to read, not one covered in controls that would
    /// refuse them — the same rule the browser follows.
    func testAViewerIsNotOfferedTheControls() {
        let app = launch("viewer")
        expect(app.buttons["item.Milk"])

        XCTAssertFalse(app.textFields["add.field"].exists, "a viewer was offered the add field")
        XCTAssertFalse(app.checkBoxes["cross.Milk"].isEnabled, "a viewer could cross off")
        XCTAssertFalse(app.buttons["item.Milk"].isEnabled, "a viewer could open the editor")
        XCTAssertFalse(app.descendants(matching: .any)["clear.done"].exists)
    }

    // MARK: - Saying what is not shown

    /// A prefix presented as the whole list makes the rows that did not fit look
    /// deleted rather than merely elsewhere.
    func testALongListSaysItIsTruncated() {
        let app = launch("truncated")
        expect(app.staticTexts["truncation.notice"], "a truncated list said nothing")

        // A macOS static text puts its content in `value`, not `label`, unless a
        // label was set — and if one was, both are worth accepting. Taking whichever
        // has something in it keeps this about what the notice says.
        let notice = app.staticTexts["truncation.notice"]
        let said = notice.label.isEmpty ? (notice.value as? String ?? "") : notice.label

        XCTAssertTrue(
            said.contains("340"),
            "did not say how many there are: label=\(notice.label) value=\(String(describing: notice.value))"
        )
    }

    func testAShortListSaysNothing() {
        let app = launch()
        expect(app.buttons["item.Milk"])

        XCTAssertFalse(app.staticTexts["truncation.notice"].exists)
    }
}
