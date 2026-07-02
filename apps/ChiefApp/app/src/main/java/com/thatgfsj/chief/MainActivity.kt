package com.thatgfsj.chief

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Surface
import androidx.compose.ui.Modifier
import com.thatgfsj.chief.ui.oracle.ChiefScreen
import com.thatgfsj.chief.ui.theme.ChiefAppTheme

/**
 * v0.1.0 (event 000070): single-activity host. The app is
 * one screen (ChiefScreen), so no NavHost is involved.
 * `viewModels()` defaults to AndroidViewModelFactory which
 * uses the no-arg ChiefViewModel constructor.
 */
class MainActivity : ComponentActivity() {

    private val viewModel: ChiefViewModel by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            ChiefAppTheme {
                Surface(modifier = Modifier.fillMaxSize()) {
                    ChiefScreen(viewModel)
                }
            }
        }
    }
}
