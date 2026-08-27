package com.cernauskas.shoppinglist

import android.app.Application
import com.cernauskas.shoppinglist.data.ServerDirectory

class ShoppingListApp : Application() {
    override fun onCreate() {
        super.onCreate()
        // Before anything asks which server, which is before anything at all: the API
        // is built from it.
        ServerDirectory.start(this)
    }
}
