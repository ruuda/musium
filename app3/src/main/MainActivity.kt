package nl.ruuda.mindec;

import android.content.Context;
import android.os.Bundle;
import android.support.v7.app.AppCompatActivity;
import android.support.v7.widget.LinearLayoutManager;
import android.support.v7.widget.RecyclerView;
import android.view.View;
import android.view.Gravity;
import android.view.ViewGroup;
import android.widget.TextView;
import android.widget.LinearLayout;
import org.jetbrains.anko.AnkoContext;
import org.jetbrains.anko.AnkoComponent;
import org.jetbrains.anko.matchParent;
import org.jetbrains.anko.padding;
import org.jetbrains.anko.dip;
import org.jetbrains.anko.margin;
import org.jetbrains.anko.textView;
import org.jetbrains.anko.imageView;
import org.jetbrains.anko.recyclerview.v7.recyclerView;
import org.jetbrains.anko.verticalLayout;
import org.jetbrains.anko.wrapContent;

data class Album
  ( val id: String
  , val title: String
  , val artist: String
  , val sortArtist: String
  , val originalReleaseDate: String
  )

class AlbumUi : AnkoComponent<ViewGroup> {

    companion object {
        const val textViewName = 1
        const val imageViewImage = 2
    }

    override fun createView(ui: AnkoContext<ViewGroup>): View = with(ui) {

        verticalLayout {
            this.orientation = LinearLayout.HORIZONTAL
            lparams(matchParent, wrapContent)
            padding = dip(16)

            imageView {
                id = imageViewImage
            }.lparams {
                height = dip(40)
                width = dip(40)
                gravity = Gravity.CENTER
            }

            textView {
                id = textViewName
                textSize = 16f
            }.lparams {
                gravity = Gravity.CENTER
                margin = dip(10)
            }
        }
    }
}

class RecyclerViewAdapter(
  private val context: Context,
  private val albums: List<Album>,
  private val listener: (Album) -> Unit
) : RecyclerView.Adapter<RecyclerViewAdapter.ViewHolder>()
{
  override fun onCreateViewHolder(parent: ViewGroup, viewType: Int): ViewHolder {
    return ViewHolder(AlbumUi().createView(AnkoContext.create(context, parent)))
  }

  override fun onBindViewHolder(holder: ViewHolder, position: Int) {
    holder.bindItem(albums[position], listener)
  }

  override fun getItemCount(): Int = albums.size

  class ViewHolder(
    val containerView: View
  ) : RecyclerView.ViewHolder(containerView)
  {
    fun bindItem(album: Album, listener: (Album) -> Unit) {
      // TODO: Render item.
      containerView.setOnClickListener { listener(album) }
    }
  }
}

class MainActivity : AppCompatActivity()
{
  private var albums: MutableList<Album> = mutableListOf();

  override fun onCreate(savedInstanceState: Bundle?) {
    super.onCreate(savedInstanceState);

    val a1 = Album(
      id = "0e16d7d764dee604",
      title = "Absolution",
      artist = "Muse",
      sortArtist = "muse",
      originalReleaseDate = "2003-09-08"
    );
    albums.add(a1);

    verticalLayout {
      lparams(matchParent, wrapContent);

      recyclerView {
        layoutManager = LinearLayoutManager(context);
        adapter = RecyclerViewAdapter(context, albums) {
          // TODO Log click.
        }
      }
    }
  }
}

