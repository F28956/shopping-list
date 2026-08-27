package com.cernauskas.shoppinglist

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import com.cernauskas.shoppinglist.data.Api
import com.cernauskas.shoppinglist.data.Cache
import com.cernauskas.shoppinglist.data.Identity
import com.cernauskas.shoppinglist.data.ServerDirectory
import com.cernauskas.shoppinglist.data.ShoppingList
import com.cernauskas.shoppinglist.ui.ItemsScreen
import com.cernauskas.shoppinglist.ui.ItemsViewModel
import com.cernauskas.shoppinglist.ui.ListsScreen
import com.cernauskas.shoppinglist.ui.ListsViewModel
import com.cernauskas.shoppinglist.ui.ServerAddressScreen
import com.cernauskas.shoppinglist.ui.ServerPeopleScreen
import com.cernauskas.shoppinglist.ui.SettingsScreen
import com.cernauskas.shoppinglist.ui.ShoppingTheme
import kotlinx.coroutines.launch

class MainActivity : ComponentActivity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)

        val identity = Identity(this)
        val api = Api(
            token = { identity.tokenNow() },
            remembered = { identity.isRemembered },
            renew = { identity.renew() },
        )
        val cache = Cache(this)

        setContent {
            ShoppingTheme {
                var state by remember { mutableStateOf<Identity.State>(Identity.State.Unknown) }
                val scope = rememberCoroutineScope()
                // Re-read after settings change the answer, because `ServerDirectory`
                // is storage and nothing observes storage.
                var hasServer by remember { mutableStateOf(ServerDirectory.hasServer) }
                /**
                 * Changing the address from the sign-in screen rather than from
                 * settings, which is behind it. See [SignIn].
                 */
                var correctingServer by remember { mutableStateOf(false) }

                // Only when there is somewhere to sign in to. Restoring asks Google,
                // and Google asks the person -- which is a sign-in sheet in front of
                // somebody who chose to keep this device to itself.
                LaunchedEffect(hasServer) {
                    if (hasServer) state = identity.restore()
                }

                // The default, and it opens straight into the lists. A shopping list
                // should be usable the moment it is installed, not open by asking a
                // question about hosting -- so there is no first-run screen, nothing
                // to dismiss, and nothing to sign in to. Somebody who has a server
                // goes and says so in settings.
                if (!hasServer) {
                    Shopping(
                        api = api,
                        cache = cache,
                        onSignedOut = {},
                        onServerChanged = { hasServer = ServerDirectory.hasServer },
                    )
                    return@ShoppingTheme
                }

                when (val current = state) {
                    Identity.State.Unknown -> Splash()

                    is Identity.State.SignedOut -> if (correctingServer) {
                        ServerAddressScreen(
                            onAccepted = { address, _ ->
                                // C4, and only when it applies: a different server
                                // mints different ids, so what the last one put in the
                                // cache cannot stay. Retyping the address already
                                // configured is a correction rather than a move, and
                                // `remember` is what tells the two apart.
                                if (ServerDirectory.remember(address)) {
                                    identity.signOut()
                                    scope.launch { cache.forgetEverything() }
                                }
                                correctingServer = false
                                hasServer = ServerDirectory.hasServer
                            },
                            onCancel = { correctingServer = false },
                        )
                    } else {
                        SignIn(
                            configured = identity.isConfigured,
                            problem = current.problem,
                            server = ServerDirectory.current?.origin,
                            onSignIn = { scope.launch { state = identity.signIn() } },
                            onChangeServer = { correctingServer = true },
                            onlyThisDevice = {
                                // C4. The lists after this belong to no server, and
                                // rows keyed by ids one minted would be its lists under
                                // nobody's name. Unlike a sign-out this is not somebody
                                // handing the phone on, but the cache goes either way:
                                // there is nothing left that could make sense of it.
                                identity.signOut()
                                scope.launch { cache.forgetEverything() }
                                ServerDirectory.onlyThisDevice()
                                hasServer = ServerDirectory.hasServer
                            },
                        )
                    }

                    is Identity.State.SignedIn -> Shopping(
                        api = api,
                        cache = cache,
                        onServerChanged = { hasServer = ServerDirectory.hasServer },
                        onSignedOut = { why ->
                            identity.signOut()
                            // Only what was asked for. A session that ended because
                            // somebody tapped Sign out belongs to a person who is
                            // leaving, and their shopping should not be waiting for
                            // whoever picks the phone up next. A session that ended
                            // because the server refused a token is not that: it is
                            // the same person with an expired credential, and throwing
                            // away their unsent changes over it would be losing work to
                            // a clock.
                            if (why is Identity.Departure.Deliberate) {
                                scope.launch { cache.forgetEverything() }
                            }
                            state = Identity.State.SignedOut(
                                (why as? Identity.Departure.Refused)?.problem
                            )
                        },
                    )
                }
            }
        }
    }
}

@Composable
private fun Splash() {
    Box(Modifier.fillMaxSize()) {
        CircularProgressIndicator(Modifier.align(Alignment.Center))
    }
}

/**
 * Signing in to the server this device has been pointed at.
 *
 * It leads back out as well as in, and it has to. This screen used to be the whole app:
 * somebody who went into settings to see what "Use a server" said arrived here, found a
 * Google button and nothing else, and the settings that had put them there were now
 * behind sign-in. A mistyped address could not be corrected and a change of mind could
 * not be acted on; reinstalling was the way out. A screen somebody cannot leave is a
 * screen that has taken the phone off them.
 */
@Composable
private fun SignIn(
    configured: Boolean,
    problem: String?,
    /** Which server, because neither way out means anything to somebody who cannot see
     * what they are leaving. */
    server: String?,
    onSignIn: () -> Unit,
    /** The address was wrong. */
    onChangeServer: () -> Unit,
    /** A server was never wanted. */
    onlyThisDevice: () -> Unit,
) {
    var leaving by remember { mutableStateOf(false) }

    Surface(Modifier.fillMaxSize()) {
        Column(
            Modifier.fillMaxSize().padding(32.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp, Alignment.CenterVertically),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Text("Shopping list", style = MaterialTheme.typography.headlineMedium)
            Text(
                "The same lists as the browser and the phone.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                textAlign = TextAlign.Center,
            )

            server?.let {
                Text(
                    it,
                    style = MaterialTheme.typography.labelLarge,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    textAlign = TextAlign.Center,
                )
            }

            if (configured) {
                Button(onClick = onSignIn) { Text("Sign in with Google") }

                // Shown where the tap happened. A refusal that says nothing is
                // indistinguishable from a button that does nothing.
                problem?.let {
                    Text(
                        it,
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.error,
                        textAlign = TextAlign.Center,
                    )
                }
            } else {
                // Said plainly rather than failing at the tap: a fresh checkout has no
                // client id, because the file holding it is not committed.
                Text(
                    "This build has no Google client id yet.\n"
                        + "Set googleWebClientId in local.properties.",
                    style = MaterialTheme.typography.bodySmall,
                    textAlign = TextAlign.Center,
                    color = MaterialTheme.colorScheme.error,
                )
            }

            // The two ways out, and they are the two mistakes anybody makes here.
            // Below the sign-in button rather than beside it: this screen is still for
            // signing in, and most people who reach it mean to.
            TextButton(onClick = onChangeServer) { Text("Use a different server") }
            TextButton(onClick = { leaving = true }) { Text("Use this device only") }
        }
    }

    // Said out loud rather than done quietly, in the same words settings uses, because
    // it is the same act.
    if (leaving) {
        AlertDialog(
            onDismissRequest = { leaving = false },
            title = { Text("Use this device only?") },
            text = {
                Text(
                    "Your lists will stay on this phone and nothing will be synced. " +
                        "This removes everything stored on this device, including " +
                        "anything still waiting to be sent. You can add a server " +
                        "again in settings."
                )
            },
            confirmButton = {
                TextButton(onClick = { leaving = false; onlyThisDevice() }) {
                    Text("Use this device only")
                }
            },
            dismissButton = {
                TextButton(onClick = { leaving = false }) { Text("Cancel") }
            },
        )
    }
}

@Composable
private fun Shopping(
    api: Api,
    cache: Cache,
    onSignedOut: (Identity.Departure) -> Unit,
    /** Settings changed which server this device uses, if any. */
    onServerChanged: () -> Unit,
) {
    val scope = rememberCoroutineScope()
    var settings by remember { mutableStateOf(false) }
    // Asked once, after the app is already usable. Nothing on the lists screen waits
    // for it -- a menu item appearing a moment late is better than a screen that waits
    // for a question about administration before it shows anybody their shopping.
    var isOwner by remember { mutableStateOf(false) }
    var managingServer by remember { mutableStateOf(false) }
    var choosingServer by remember { mutableStateOf(false) }

    LaunchedEffect(Unit) { isOwner = runCatching { api.whoAmI().isOwner }.getOrDefault(false) }

    if (managingServer) {
        ServerPeopleScreen(api = api, onDone = { managingServer = false })
        return
    }

    if (settings) {
        SettingsScreen(
            onDone = { settings = false },
            onUseServer = { settings = false; choosingServer = true },
            onLeaveServer = {
                // The order matters only in that the address goes last: if anything
                // above fails, the device is still pointed at a server it can be
                // signed into again rather than at nothing with a cache full of
                // somebody else's ids.
                scope.launch { cache.forgetEverything() }
                ServerDirectory.forget()
                settings = false
                onServerChanged()
            },
        )
        return
    }

    if (choosingServer) {
        ServerAddressScreen(
            onAccepted = { address, _ ->
                // Nothing is thrown away: adding a server is not the destructive half
                // of changing one, and what is here is about to be sent to it.
                ServerDirectory.remember(address)
                choosingServer = false
                onServerChanged()
            },
            onCancel = { choosingServer = false },
        )
        return
    }

    val nav = rememberNavController()
    var open by remember { mutableStateOf<ShoppingList?>(null) }

    NavHost(navController = nav, startDestination = "lists") {
        composable("lists") {
            val model: ListsViewModel = viewModel(
                factory = factory { ListsViewModel(api, cache, onSignedOut) },
            )
            ListsScreen(
                model = model,
                onOpen = { list -> open = list; nav.navigate("items") },
                // Deliberately signed out, so nothing to explain on the way back.
                onSignOut = { onSignedOut(Identity.Departure.Deliberate) },
                isOwner = isOwner,
                onManageServer = { managingServer = true },
                onSettings = { settings = true },
                onDeviceOnly = ServerDirectory.isOnDeviceOnly,
            )
        }

        composable("items") {
            val list = open
            if (list == null) {
                // Nothing to show: the only way here is through a list, so this is a
                // process death that took the selection with it.
                LaunchedEffect(Unit) { nav.popBackStack() }
                return@composable
            }

            val model: ItemsViewModel = viewModel(
                key = "items-${list.id}",
                factory = factory { ItemsViewModel(api, cache, list, onSignedOut) },
            )
            ItemsScreen(model = model, onBack = { nav.popBackStack() })
        }
    }
}

/** A view model factory from a lambda, so a screen can build one with what it needs
 * without a dependency-injection framework for four objects. */
private fun <T : ViewModel> factory(build: () -> T) = object : ViewModelProvider.Factory {
    @Suppress("UNCHECKED_CAST")
    override fun <V : ViewModel> create(modelClass: Class<V>): V = build() as V
}
