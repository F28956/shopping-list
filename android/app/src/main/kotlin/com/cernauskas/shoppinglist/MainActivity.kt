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
import com.cernauskas.shoppinglist.data.ShoppingList
import com.cernauskas.shoppinglist.ui.ItemsScreen
import com.cernauskas.shoppinglist.ui.ItemsViewModel
import com.cernauskas.shoppinglist.ui.ListsScreen
import com.cernauskas.shoppinglist.ui.ListsViewModel
import com.cernauskas.shoppinglist.ui.ShoppingTheme
import kotlinx.coroutines.launch

class MainActivity : ComponentActivity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)

        val identity = Identity(this)
        val api = Api(token = { identity.current() })
        val cache = Cache(this)

        setContent {
            ShoppingTheme {
                var state by remember { mutableStateOf<Identity.State>(Identity.State.Unknown) }
                val scope = rememberCoroutineScope()

                LaunchedEffect(Unit) { state = identity.restore() }

                when (val current = state) {
                    Identity.State.Unknown -> Splash()

                    is Identity.State.SignedOut -> SignIn(
                        configured = identity.isConfigured,
                        problem = current.problem,
                        onSignIn = { scope.launch { state = identity.signIn() } },
                    )

                    is Identity.State.SignedIn -> Shopping(
                        api = api,
                        cache = cache,
                        onSignedOut = { problem ->
                            identity.signOut()
                            // What was cached belongs to whoever just signed out. The
                            // next person to use this phone is a different person.
                            scope.launch { cache.forgetEverything() }
                            state = Identity.State.SignedOut(problem)
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

@Composable
private fun SignIn(configured: Boolean, problem: String?, onSignIn: () -> Unit) {
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
        }
    }
}

@Composable
private fun Shopping(api: Api, cache: Cache, onSignedOut: (String?) -> Unit) {
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
                onSignOut = { onSignedOut(null) },
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
