package app.photocurator.next

import app.photocurator.next.Prepare.ListingDecision
import org.junit.Assert.assertEquals
import org.junit.Test

class ListingDecisionTest {
    private fun photo(rel: String) =
        Photo(id = 1, name = rel, relativePath = rel, size = 1, takenAt = 1)

    @Test fun 新しい顔ぶれがあれば置き換える() {
        assertEquals(ListingDecision.Replace, Prepare.decideListing(null, listOf(photo("a"))))
        assertEquals(ListingDecision.Replace, Prepare.decideListing(listOf(photo("a")), listOf(photo("b"))))
    }

    @Test fun 前があるのに空で返ったら前を残す() {
        // 失敗が空に見えた形。上書きするとハッシュ値まで消える。
        assertEquals(ListingDecision.KeepPrevious, Prepare.decideListing(listOf(photo("a")), emptyList()))
    }

    @Test fun 元から無くて空なら書かない() {
        assertEquals(ListingDecision.Skip, Prepare.decideListing(null, emptyList()))
        assertEquals(ListingDecision.Skip, Prepare.decideListing(emptyList(), emptyList()))
    }
}
