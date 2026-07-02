package com.thatgfsj.chief

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Surface
import androidx.compose.ui.Modifier
import com.thatgfsj.chief.ui.oracle.TarotScreen
import com.thatgfsj.chief.ui.theme.ChiefAppTheme

class MainActivity : ComponentActivity() {
    private val viewModel: TarotViewModel by viewModels()
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            ChiefAppTheme {
                Surface(modifier = Modifier.fillMaxSize()) {
                    TarotScreen(viewModel)
                }
            }
        }
    }
}
