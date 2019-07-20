package nl.ruuda.mindec;

import android.app.Activity;
import android.os.Bundle;
import android.widget.TextView;

class MainActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState);

        val label = TextView(this);
        label.setText("Mindec");

        setContentView(label);
    }
}

