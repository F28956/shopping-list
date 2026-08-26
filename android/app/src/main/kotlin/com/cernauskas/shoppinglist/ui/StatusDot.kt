package com.cernauskas.shoppinglist.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp

/**
 * Whether this device is in step with the server, as one dot.
 *
 * The same two colours the watch uses, for the same reason: it is the one signal you
 * can read without stopping. The phone keeps its sentence as well — it has a line to
 * spare and the line says how many and why — but the sentence only appears when
 * something is wrong, and a dot that is green the rest of the time is the difference
 * between "nothing is wrong" and "nothing has been checked".
 *
 * * **Green** — what is on screen came from the server and nothing is waiting to go
 *   back.
 * * **Orange** — one of those is not true: either something you did is still queued, or
 *   the last look at the server failed and this is from memory.
 */
@Composable
fun StatusDot(waiting: Int, offline: Boolean, modifier: Modifier = Modifier) {
    val inStep = waiting == 0 && !offline

    androidx.compose.foundation.layout.Box(
        modifier
            .padding(horizontal = 8.dp)
            .size(9.dp)
            .background(
                // Not from the theme: green and orange are the point, and a colour
                // scheme that happened to make them similar would take the signal away.
                if (inStep) Color(0xFF34C759) else Color(0xFFFF9500),
                CircleShape,
            )
            .semantics { contentDescription = said(waiting, offline) },
    )
}

/**
 * Spoken in full, because what makes a dot right at a glance is exactly what makes it
 * useless to somebody reading by ear.
 */
private fun said(waiting: Int, offline: Boolean): String = when {
    !offline && waiting == 0 -> "Up to date"
    waiting == 0 -> "Offline. Showing what was last loaded."
    waiting == 1 -> "1 change waiting to be sent"
    else -> "$waiting changes waiting to be sent"
}
