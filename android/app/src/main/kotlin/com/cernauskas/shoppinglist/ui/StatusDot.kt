package com.cernauskas.shoppinglist.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import com.cernauskas.shoppinglist.data.LocalCapabilities
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
 *
 * **Absent on a device kept to itself**, which is a change: this used to show green and
 * say "On this device only". Green is a claim that a connection is healthy, and there is
 * no connection — the dot would be permanently, unfailingly green, which is an indicator
 * somebody learns to ignore, and that is worse than no indicator. The Apple apps hide it
 * for exactly this reason and said so in a comment while this showed it anyway.
 */
@Composable
fun StatusDot(
    waiting: Int,
    offline: Boolean,
    modifier: Modifier = Modifier,
) {
    // Not "is there a server": is there a far end that can be out of reach. The two
    // agree today and are different questions -- see `Capabilities`.
    if (!LocalCapabilities.current.syncing) return

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
