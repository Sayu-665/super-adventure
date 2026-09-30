package com.opensurf.browser;

import android.app.AlertDialog;
import android.view.LayoutInflater;
import android.view.View;
import android.view.ViewGroup;
import android.widget.BaseAdapter;
import android.widget.ListView;
import android.widget.TextView;

import java.util.List;

/** Dialog listing the open tabs: tap to switch, X to close, "New tab" to add one. */
final class TabSwitcher {
    private TabSwitcher() {
    }

    static void show(MainActivity activity) {
        ListView list = new ListView(activity);
        list.setDivider(null);
        int padding = Math.round(8 * activity.getResources().getDisplayMetrics().density);
        list.setPadding(0, padding, 0, padding);
        list.setClipToPadding(false);
        TabAdapter adapter = new TabAdapter(activity);
        list.setAdapter(adapter);

        AlertDialog.Builder builder = new AlertDialog.Builder(activity)
                .setTitle(title(activity))
                .setView(list)
                .setPositiveButton(R.string.tabs_new, (d, w) -> activity.openNewTabFromUi())
                .setNegativeButton(R.string.tabs_done, null);
        AlertDialog dialog = activity.showDialog(builder, null);

        list.setOnItemClickListener((parent, view, position, id) -> {
            activity.switchTo(adapter.getItem(position));
            dialog.dismiss();
        });
        adapter.onClose = tab -> {
            activity.closeTab(tab);
            adapter.notifyDataSetChanged();
            dialog.setTitle(title(activity));
        };
        list.setSelection(Math.max(0, activity.tabs().indexOf(activity.currentTab())));
    }

    private static String title(MainActivity activity) {
        return activity.getString(R.string.tabs_title, activity.tabs().size());
    }

    private interface CloseListener {
        void onClose(BrowserTab tab);
    }

    private static final class TabAdapter extends BaseAdapter {
        private final MainActivity activity;
        private final List<BrowserTab> tabs;
        CloseListener onClose;

        TabAdapter(MainActivity activity) {
            this.activity = activity;
            this.tabs = activity.tabs();
        }

        @Override
        public int getCount() {
            return tabs.size();
        }

        @Override
        public BrowserTab getItem(int position) {
            return tabs.get(position);
        }

        @Override
        public long getItemId(int position) {
            return System.identityHashCode(tabs.get(position));
        }

        @Override
        public View getView(int position, View convertView, ViewGroup parent) {
            View row = convertView != null ? convertView
                    : LayoutInflater.from(activity).inflate(R.layout.tab_item, parent, false);
            BrowserTab tab = getItem(position);
            TextView title = row.findViewById(R.id.tab_title);
            TextView url = row.findViewById(R.id.tab_url);
            View close = row.findViewById(R.id.tab_close);

            boolean home = tab.isHome() || tab.url.isEmpty();
            String label = !tab.title.isEmpty() && !home ? tab.title
                    : home ? activity.getString(R.string.new_tab_title) : tab.url;
            title.setText(label);
            url.setText(home ? "" : tab.url);
            url.setVisibility(home ? View.GONE : View.VISIBLE);
            row.setActivated(tab == activity.currentTab());
            close.setOnClickListener(v -> {
                if (onClose != null) {
                    onClose.onClose(tab);
                }
            });
            return row;
        }
    }
}
