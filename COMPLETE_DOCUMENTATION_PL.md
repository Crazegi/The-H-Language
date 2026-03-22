H — Proste wyjaśnienie projektu (język polski, bardzo prosto)
=============================================================

Cel pliku
--------
To jest prosty przewodnik po projekcie „H”. Jest napisany bardzo prosto, tak aby osoba bez doświadczenia w kompilatorach czy Rust mogła zrozumieć co to robi i dlaczego to jest użyteczne.

Co to w ogóle jest?
--------------------
H to mały język programowania stworzony do pracy z precyzyjnymi, krótkimi sekwencjami kodu — np. wysyłaniem sygnałów do urządzeń (pulsów). Ma dwie ważne cechy:
- czytelna struktura oparta na wcięciach (jak w Pythonie),
- możliwość deklarowania „kontraktów czasowych” (cycle contracts) — czyli gwarancji, ile cykli (kroków) zajmie fragment kodu.

Prosty opis ścieżki pracy (pipeline)
-----------------------------------
1. Lexer — przetwarza tekst programu na „tokeny” (elementy jak słowa, liczby, znaki). Wyłapuje wcięcia.
2. Parser — zamienia tokeny w drzewo programu (AST), np. funkcje, instrukcje.
3. Analiza semantyczna — sprawdza poprawność typów i reguł (np. że `ref` nie jest zapisywane).
4. Kompilator — zamienia drzewo na prosty kod bajtowy (bytecode) i liczy zużycie cykli w kontraktach.
5. VM — uruchamia ten bajtowy kod w prostym środowisku.
6. (Opcjonalnie) native — generuje Rustowy program, który można skompilować jako natywny binarny plik.
7. Pakowanie — zapisuje skompilowany bytecode do pliku `.hbcp` do łatwego uruchamiania bez kompilatora Rust.

Jak uruchomić (proste polecenia)
--------------------------------
- Uruchom testy:

```powershell
cargo test
```

- Skompiluj źródło do bytecode i uruchom w maszynie wirtualnej:

```powershell
cargo run --bin hl-lex -- --compile examples/cycle_contracts.hl --out out.hbc.txt
cargo run --bin hl-lex -- --vm examples/cycle_contracts.hl
```

Główne pliki (krótko)
---------------------
- `src/lexer.rs` — dzieli tekst na tokeny, obsługuje wcięcia i klucze YAML-owe.
- `src/token.rs` — lista rodzajów tokenów (np. `Indent`, `Number`, `Colon`).
- `src/parser.rs` — tworzy strukturę programu (funkcje, instrukcje).
- `src/ast.rs` — struktury drzewa (Expr, Stmt, Function).
- `src/semantic.rs` — sprawdza zasady (typy, reguły `ref`/`own`).
- `src/compiler.rs` — zamienia AST na bytecode i pilnuje kontraktów czasowych.
- `src/bytecode.rs` — definicja instrukcji, np. `Add`, `Mov`, `Nop`.
- `src/vm.rs` — prosty silnik wykonujący bytecode.
- `src/package.rs` — zapis/odczyt pliku `.hbcp`.
- `src/main.rs` — program uruchamiający (CLI).

Co to są "kontrakty czasowe" (cycle contracts)? — wyjaśnienie prosto
--------------------------------------------------------------------
Wyobraź sobie, że chcesz wysłać sekwencję sygnałów do diody albo złącza i musisz, żeby trwało to dokładnie 10 „kroków”. Możesz napisać:

```
contract:
  cycles: 10
  on_underflow: "pad_nop"
  on_overflow: "compile_error"
execute:
  mov [port], r1
  add r1, r2
```

- Kompilator policzy ile „cykli” zajmują instrukcje.
- Jeśli jest za mało cykli i `on_underflow == pad_nop`, to program wstawia `Nop` (puste kroki), by dopasować długość.
- Jeśli jest za dużo i `on_overflow == compile_error`, kompilacja się nie powiedzie — masz to od razu, przed uruchomieniem.

Dlaczego to jest przydatne?
- W systemach wbudowanych (embedded) trzeba mieć dokładny czas działania dla bezpieczeństwa i synchronizacji.
- Ta funkcja pozwala wykryć błędy w czasie kompilacji, zanim trafi do urządzenia.

Jak pisać programy — kilka prostych zasad
---------------------------------------
- Bloki kodu: używaj dwukropka i wcięcia, np. `fn main():` i wcięty kod poniżej.
- Nie używamy nawiasów klamrowych `{}` — projekt wybrał jedną prostą składnię.
- Do wpisania wielu pól do wydruku użyj `print:` i pod nim wciętych kluczy YAML-owych.

Przykład prostego programu
--------------------------

```
section .data:
  name: "Sensor"

section .text:
  fn main():
    own r1 = 5
    add r1, 3
    return r1
```

Proste analogie (dla zrozumienia)
---------------------------------
- Lexer to taki czytnik słów w tekście — jak skaner w sklepie rozpoznający produkty.
- Parser układa te słowa w sensowne zdania — jak składanie instrukcji z części.
- Kompilator to tłumacz na prosty język maszynowy (bytecode), który rozumie nasza VM.
- Cycle contract to umowa: "to musi trwać N kroków" — kompilator pilnuje tej umowy.

Czego unikać
------------
- Nie mieszaj stylów składni (np. klamer i wcięć) — projekt używa tylko wcięć.
- Nie używaj tabulatorów do wcięć — lexer odrzuca taby.

Dalsze kroki (jeśli chcesz rozwinąć)
------------------------------------
- Jeśli chcesz pełne opisy funkcja-po-funkcji, mogę dodać komentarze dokumentacyjne (po polsku) do każdego pliku w `src/`.
- Mogę też przygotować wersję "krok po kroku" z przykładami edycji i uruchamiania na Windowsie lub Linuxie.

Podsumowanie prosto
-------------------
H to lekki język do pisania krótkich, deterministycznych programów, z narzędziami, które:
- sprawdzają poprawność kodu,
- gwarantują czasy wykonania dla krytycznych fragmentów,
- pozwalają uruchamiać programy na prostym VM lub zbudować natywny binarkę.

Jeśli chcesz, doprecyzuj: czy mam teraz dodać szczegółowe opisy każdej funkcji w `src/` (po polsku), czy wolisz krótsze instrukcje dla użytkownika krok-po-kroku?