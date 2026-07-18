# Build-time Math

Peitho renders fenced `math` blocks during the build.

```math
\int_0^1 x^2\,dx = \frac{1}{3}
```

---

# Matrices

The source remains LaTeX in the deck and manifest text.

```math
\begin{pmatrix}
a & b \\
c & d
\end{pmatrix}^{-1}
=
\frac{1}{ad-bc}
\begin{pmatrix}
d & -b \\
-c & a
\end{pmatrix}
```

---

# Limits

No client-side math JavaScript is needed.

```math
\lim_{n \to \infty}\sum_{k=1}^{n}\frac{1}{n}
=
\int_0^1 1\,dx
= 1
```
