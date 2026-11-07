---
time: 3m
public_reveal: steps
---

<!-- {"layout":"cover"} -->

# Hello, Vue

---

<!-- {"layout":"poll"} -->

# Which do you reach for?

- Vue
- React
- Svelte
- Plain DOM

---

<!-- {"layout":"morph"} -->

# Options API to `<script setup>`

```js
export default {
  data() {
    return { count: 0 }
  },
  methods: {
    increment() {
      this.count++
    }
  }
}
```

```js
import { ref } from 'vue'

export default {
  setup() {
    const count = ref(0)
    const increment = () => count.value++
    return { count, increment }
  }
}
```

```js
import { ref, computed } from 'vue'

const count = ref(0)
const double = computed(() => count.value * 2)
const increment = () => count.value++
```

---

<!-- {"layout":"marker"} -->

# Say it with a marker

::: {reveal}

- Layouts own the **design**
- Markdown owns the **content**
- Reveal steps move **the pen**
- The Markdown **never changes**

:::

---

<!-- {"layout":"motion"} -->

# Spring into view

- Reactive
- Composable
- Animated
- Typed

---

<!-- {"layout":"types"} -->

# Types on stage

```ts
import { ref, computed } from 'vue'

const count = ref(0)
//    ^?

const label = computed(() => `${count.value} clicks`)
//    ^?

```

---

<!-- {"layout":"plain"} -->

# How it works

- The Markdown holds only content: titles, lists, and code blocks.
- Each layout's `<script type="module">` imports a library and mounts on its slide.
- Vue: `v-model`, `<TransitionGroup>`, and `v-motion`.
- Shiki Magic Move and Twoslash highlight code in the browser.
- Rough Notation marks **bold** words as reveal steps show them.
- Everything loads from a CDN, so the deck needs network access.
