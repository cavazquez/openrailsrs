# Perfil compuesto propio y calderas independientes

`examples/physics_lab/vehicles/compound.eng` declara una extensión propia con
cilindros HP/LP, receptor finito, dimensiones y válvula de arranque. Las
calderas de la formación conservan su identidad, inventarios, mandos y esfuerzo
por coche; los reguladores locales respetan los interbloqueos de tracción.
Los ensayos dirigidos11 registraron14/14 pruebas físicas propias, incluido
reparto/consumo/calor por locomotora y continuidad de guardado. Ese resultado
no es una medición del compuesto original de Open Rails.

El perfil original1.6.1 modela Vauclain balanceado sin receptor. No se afirma
que este receptor represente cualquier ENG `Compound`, ni que una captura de
una locomotora simple mida el ciclo compuesto. Los criterios del perfil propio
se evalúan como extensión declarada.

El recipiente de la caldera tiene balance de masa y energía. El receptor
registra admisión HP, salida LP y energía advectada; descuenta el trabajo HP
de la energía admitida. Su presión usa gas ideal a temperatura de caldera.
La energía que sale se registra antes de la expansión LP; el trabajo LP figura
en un contador separado. No es una solución completa de energía, expansión,
condensación y temperatura del receptor/LP.

Con velocidad distinta de cero, las fuerzas HP y LP están limitadas por la
masa disponible y la entalpía utilizable por paso; los límites de esfuerzo y
la falta de vapor escalan las dos fuerzas. A velocidad cero puede existir
esfuerzo estático y el trabajo mecánico es cero. El cierre de regulador anula
la fuerza y el receptor se drena con la constante propia de 10 s. Estas son
garantías del modelo declarado, no equivalencia con OR.

La revisión corrigió un borde contable: dividir trabajo por
`max(fuerza_total,1 N)` creaba trabajo LP ficticio cuando la fuerza total era
menor que 1 N y sólo HP producía esfuerzo. El cálculo ahora usa directamente
`fuerza_HP × desplazamiento` y `fuerza_LP × desplazamiento`, conservando los
límites físicos. No cambia fuerza, consumo ni tolerancias. Sus regresiones
unitarias de 0,5 N y bypass
[pasaron en libs-and-mp03](fixtures/compatibility/twenty-20261015/steam-cycle161-own-ledger03/receipt.json).
El contador HP representa trabajo descontado de vapor encaminado al receptor;
el bypass queda fuera de ese contador y no se presenta como trabajo HP total.
Las 14 pruebas físicas
dirigidas11 sí pasaron, incluido reparto/consumo por identidad y conducción
guardada del doble vapor.
