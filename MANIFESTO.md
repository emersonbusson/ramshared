# Manifesto RamShared

A VRAM de computadores pessoais e workstations frequentemente passa 90% do tempo ociosa em tarefas não gráficas, enquanto os limites de RAM são atingidos constantemente, causando swap excessivo para discos dezenas de vezes mais lentos que a VRAM.

**Nosso objetivo é tratar todo o silício com a máxima eficiência possível, quebrando a barreira artificial entre RAM e VRAM em nível de sistema operacional.**

## Princípios

1. **Bare-Metal First**
   Evitamos a sobrecarga de context switching. Sempre que possível, a solução deve operar na camada mais baixa possível sem destruir o sistema. Preferimos HMM, NUMA nodes e CXL a emuladores em userspace.

2. **Previsibilidade acima de Hacks**
   Um kernel panic causado por ponteiro pendente na VRAM é inaceitável. O código deve ser rigorosamente auditado contra falhas em estados de energia de hardware (D3hot/D3cold) e reset repentino de GPU.

3. **Respeito à Fila de I/O**
   A comunicação com hardware via PCIe deve respeitar restrições de latência. O bloqueio de I/O por polling em spinlocks é desencorajado. DMA (Direct Memory Access) deve ser o padrão ouro.

4. **Kahneman System 2 for Kernel**
   No nível do ring 0 (kernel space), não há espaço para tentativas "trial and error". Cada alteração estrutural deve responder explicitamente aos riscos de interrupção, page fault handling, e memory leak. Acreditamos que o código de kernel requer uma reflexão estruturada e metódica antes de qualquer compilação.
