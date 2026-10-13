"""Part B: edge and compute. Variants of the generic kinds and the new product kinds."""
from arch import *

# ---- variants of the generic kinds (they extend catalog_gen_a.VARIANTS) -------------------------------------------------
VARIANTS_B = {
    "gslb": [
        variant("self-hosted", "Свои DNS-серверы (PowerDNS / BIND + проверки)", "Own DNS servers (PowerDNS / BIND + health checks)",
                "Авторитативные серверы на своих площадках с проверкой доступности; за их отказоустойчивость отвечаете вы.", "Authoritative servers on your own sites with health checks; making them resilient is up to you.",
                default=True, defaults={"hosting": "vm"}),
        variant("route53", "Amazon Route 53", "Amazon Route 53", "Проверки состояния, маршрутизация по задержке, геолокации и весам; серверы имён у провайдера.",
                "Health checks, latency, geolocation and weighted routing; the name servers are the provider’s.", add_traits=["managed"], defaults={"hosting": "managed"}),
        variant("cloudflare", "Cloudflare Load Balancing", "Cloudflare Load Balancing", "Глобальная балансировка и проверки на сети Cloudflare, трафик может идти через их прокси.",
                "Global load balancing and monitors on the Cloudflare network; traffic can pass through their proxy.", add_traits=["managed"], defaults={"hosting": "managed"}),
        variant("cloud-dns", "Другой облачный DNS / Traffic Manager", "Other managed DNS / traffic manager",
                "Google Cloud DNS, Azure Traffic Manager, Yandex Cloud DNS и аналоги.", "Google Cloud DNS, Azure Traffic Manager, Yandex Cloud DNS and the like.", add_traits=["managed"], defaults={"hosting": "managed"}),
    ],
    "cdn": [
        variant("cloudflare", "Cloudflare", "Cloudflare", "Сеть доставки с кэшем, DDoS-защитой и WAF на границе.", "A delivery network with a cache, DDoS protection and a WAF at the edge.", default=True, add_traits=["managed"], defaults={"hosting": "managed"}),
        variant("cloudfront", "Amazon CloudFront", "Amazon CloudFront", "CDN AWS с группами источников: при отказе первого источника запрос уходит ко второму.", "The AWS CDN with origin groups: when the first origin fails the request goes to the second.", add_traits=["managed"], defaults={"hosting": "managed"}),
        variant("akamai", "Akamai", "Akamai", "Крупнейшая CDN с собственной сетью точек присутствия.", "A large CDN with its own network of points of presence.", add_traits=["managed"], defaults={"hosting": "managed"}),
        variant("fastly", "Fastly", "Fastly", "CDN с программируемым краем (VCL / Compute).", "A CDN with a programmable edge (VCL / Compute).", add_traits=["managed"], defaults={"hosting": "managed"}),
        variant("regional", "Региональная CDN (Gcore, Yandex Cloud CDN, Selectel…)", "Regional CDN (Gcore, Yandex Cloud CDN, Selectel…)", "Провайдер с точками присутствия рядом с вашей аудиторией.", "A provider with points of presence close to your audience.", add_traits=["managed"], defaults={"hosting": "managed"}),
        variant("self-hosted", "Свои кэширующие узлы (Varnish / NGINX)", "Own caching nodes (Varnish / NGINX)", "Собственные точки присутствия: вы отвечаете за каждую площадку и за маршрутизацию на ближайшую.", "Your own points of presence: every site and the routing to the nearest one are yours.", defaults={"hosting": "vm"}),
    ],
    "waf": [
        variant("cloud", "Облачный WAF (Cloudflare, AWS WAF, Imperva…)", "Cloud WAF (Cloudflare, AWS WAF, Imperva…)", "Правила и обновления сигнатур у провайдера, фильтрация до вашей сети.", "Rules and signature updates at the provider, filtering before your network.", default=True, add_traits=["managed"], defaults={"hosting": "managed"}),
        variant("modsecurity", "ModSecurity / Coraza + OWASP CRS", "ModSecurity / Coraza + OWASP CRS", "Открытый движок поверх NGINX, Envoy или HAProxy с набором правил OWASP.", "An open engine on top of NGINX, Envoy or HAProxy with the OWASP rule set.", defaults={"hosting": "vm"}),
        variant("nginx-app-protect", "NGINX App Protect", "NGINX App Protect", "Коммерческий WAF в составе NGINX Plus.", "A commercial WAF built into NGINX Plus.", defaults={"hosting": "vm"}),
        variant("appliance", "Аппаратный / виртуальный (F5, FortiWeb, Imperva)", "Appliance (F5, FortiWeb, Imperva)", "Выделенные устройства в паре; обновление правил и лицензии — на вас.", "Dedicated devices in a pair; rule updates and licences are yours.", defaults={"hosting": "vm"}),
    ],
    "l7": [
        variant("vrrp", "На своих серверах (VRRP / keepalived)", "Own servers (VRRP / keepalived)", "Пара узлов с плавающим адресом: переключение за секунды, трафик идёт через активный узел.", "A pair of nodes with a floating address: failover in seconds, traffic goes through the active node.", default=True, defaults={"hosting": "vm"}),
        variant("ingress-k8s", "Ingress-контроллер в Kubernetes", "Ingress controller on Kubernetes", "Реплики контроллера в кластере за балансировщиком уровня L4; масштабируется вместе с кластером.", "Controller replicas on the cluster behind an L4 balancer; scales with the cluster.", defaults={"hosting": "kubernetes"}),
        variant("cloud-alb", "Облачный L7 (ALB, Application Gateway, Cloud Load Balancing)", "Cloud L7 (ALB, Application Gateway, Cloud Load Balancing)", "Провайдер держит узлы в нескольких зонах и сам заменяет неисправные.", "The provider keeps nodes in several zones and replaces failed ones.", add_traits=["managed"], defaults={"hosting": "managed"}),
        variant("appliance", "Аппаратный / виртуальный (F5, Citrix ADC, A10)", "Appliance (F5, Citrix ADC, A10)", "Выделенные устройства в паре active/standby с синхронизацией состояния сессий.", "Dedicated devices in an active/standby pair with session state sync.", defaults={"hosting": "vm"}),
    ],
    "l4": [
        variant("vrrp-lvs", "VRRP + LVS / HAProxy", "VRRP + LVS / HAProxy", "Пара узлов с плавающим адресом, балансировка на уровне TCP/UDP.", "A pair of nodes with a floating address, TCP/UDP balancing.", default=True, defaults={"hosting": "vm"}),
        variant("anycast-bgp", "Anycast + BGP (ECMP)", "Anycast + BGP (ECMP)", "Один адрес анонсируется со многих узлов, сеть сама раскладывает потоки и убирает отказавшие.", "One address is announced from many nodes, the network spreads the flows and drops failed ones.", defaults={"hosting": "vm"}),
        variant("cloud-nlb", "Облачный L4 (NLB, Load Balancer)", "Cloud L4 (NLB, Load Balancer)", "Провайдер держит узлы в нескольких зонах.", "The provider keeps nodes in several zones.", add_traits=["managed"], defaults={"hosting": "managed"}),
    ],
    "vpn_dci": [
        variant("ipsec", "IPsec site-to-site", "IPsec site-to-site", "Шифрованный туннель через интернет между шлюзами (strongSwan, аппаратные шлюзы). Нужны два независимых туннеля.", "An encrypted tunnel over the internet between gateways (strongSwan, appliances). Two independent tunnels are needed.", default=True),
        variant("wireguard", "WireGuard", "WireGuard", "Лёгкий туннель на ядре Linux; переключение между путями остаётся на маршрутизации.", "A light tunnel in the Linux kernel; switching between paths is left to routing."),
        variant("openvpn", "OpenVPN", "OpenVPN", "Туннель в пользовательском пространстве; удобен для клиентов и небольших каналов.", "A userspace tunnel; handy for clients and small links.", versions="openvpn"),
        variant("cloud-vpn", "Облачный VPN (AWS Site-to-Site, Cloud VPN, VPN Gateway)", "Cloud VPN (AWS Site-to-Site, Cloud VPN, VPN Gateway)", "Два туннеля на стороне провайдера в разных зонах.", "Two tunnels on the provider’s side in different zones.", add_traits=["managed"]),
        variant("dedicated", "Выделенный канал (Direct Connect, ExpressRoute, оптика, L2)", "Dedicated link (Direct Connect, ExpressRoute, dark fibre, L2)", "Физический канал с гарантированной полосой; для отказоустойчивости нужны два канала по разным трассам.", "A physical link with a guaranteed bandwidth; for resilience two links over different routes are needed."),
        variant("sdwan", "MPLS / SD-WAN", "MPLS / SD-WAN", "Оператор или оверлей выбирает путь по качеству канала.", "A carrier or an overlay picks the path by link quality."),
    ],
    "vm_pool": [
        variant("vsphere", "VMware vSphere (ESXi)", "VMware vSphere (ESXi)", "Кластер ESXi с vSphere HA: упавшие виртуальные машины запускаются на живых узлах.", "An ESXi cluster with vSphere HA: failed virtual machines restart on live hosts.", default=True, versions="esxi"),
        variant("proxmox", "Proxmox VE", "Proxmox VE", "KVM и LXC с кластером Corosync и HA-менеджером: нужен кворум из 3 узлов.", "KVM and LXC with a Corosync cluster and an HA manager: a quorum of 3 nodes is needed.", versions="proxmox_ve"),
        variant("xcpng", "XCP-ng", "XCP-ng", "Открытая платформа на Xen с пулом и HA.", "An open Xen-based platform with pools and HA.", versions="xcp_ng"),
        variant("ovirt", "oVirt / RHV", "oVirt / RHV", "KVM с централизованным управлением и перезапуском машин при отказе узла.", "KVM with central management and restart of machines when a host fails.", versions="ovirt"),
        variant("nutanix", "Nutanix AHV", "Nutanix AHV", "Гиперконвергентная платформа: данные хранятся с репликацией между узлами.", "A hyper-converged platform: data is stored with replication between nodes.", versions="nutanix-aos"),
        variant("kvm", "KVM / libvirt", "KVM / libvirt", "Свои гипервизоры без оркестратора: перезапуск виртуальных машин — на вас.", "Plain hypervisors with no orchestrator: restarting machines is up to you.", versions=""),
        variant("cloud", "Облачные виртуальные машины (EC2, Compute Engine, Yandex Compute…)", "Cloud virtual machines (EC2, Compute Engine, Yandex Compute…)", "Группы автомасштабирования по зонам доступности.", "Auto-scaling groups across availability zones.", add_traits=["managed"], defaults={"hosting": "managed"}, versions=""),
    ],
    "functions": [
        variant("cloud", "Облачные функции (Lambda, Cloud Functions, Yandex Cloud Functions)", "Cloud functions (Lambda, Cloud Functions, Yandex Cloud Functions)", "Провайдер запускает и масштабирует экземпляры в нескольких зонах.", "The provider runs and scales instances across zones.", default=True, add_traits=["managed"], defaults={"hosting": "managed"}),
        variant("knative", "Knative", "Knative", "Serverless поверх Kubernetes: масштабирование до нуля по запросам.", "Serverless on Kubernetes: scale to zero on requests.", defaults={"hosting": "kubernetes"}),
        variant("openfaas", "OpenFaaS", "OpenFaaS", "Функции в контейнерах на Kubernetes или одиночном узле.", "Functions in containers on Kubernetes or a single host.", defaults={"hosting": "kubernetes"}),
    ],
    "api_gateway": [
        variant("self-hosted", "Свой шлюз (Envoy, Tyk, KrakenD…)", "Own gateway (Envoy, Tyk, KrakenD…)", "Несколько реплик за балансировщиком; конфигурация из Git.", "Several replicas behind a balancer; configuration from Git.", default=True),
        variant("cloud", "Облачный шлюз (API Gateway, Azure APIM, Apigee)", "Cloud gateway (API Gateway, Azure APIM, Apigee)", "Провайдер держит шлюз в нескольких зонах.", "The provider keeps the gateway in several zones.", add_traits=["managed"], defaults={"hosting": "managed"}),
    ],
    "users": [],
}

# secondary version props (OS of a virtual machine, runtime of an application)
OS_OPTIONS = [("ubuntu", "Ubuntu", "Ubuntu"), ("debian", "Debian", "Debian"), ("rhel", "Red Hat Enterprise Linux", "Red Hat Enterprise Linux"), ("rocky-linux", "Rocky Linux", "Rocky Linux"),
              ("almalinux", "AlmaLinux", "AlmaLinux"), ("oracle-linux", "Oracle Linux", "Oracle Linux"), ("sles", "SUSE Linux Enterprise", "SUSE Linux Enterprise"), ("windows-server", "Windows Server", "Windows Server"),
              ("alpine", "Alpine Linux", "Alpine Linux"), ("amazon-linux", "Amazon Linux", "Amazon Linux")]
RUNTIME_OPTIONS = [("nodejs", "Node.js", "Node.js"), ("python", "Python", "Python"), ("eclipse-temurin", "Java (Eclipse Temurin)", "Java (Eclipse Temurin)"), ("go", "Go", "Go"), ("dotnet", ".NET", ".NET"),
                   ("php", "PHP", "PHP"), ("ruby", "Ruby", "Ruby"), ("spring-boot", "Spring Boot", "Spring Boot"), ("tomcat", "Apache Tomcat", "Apache Tomcat")]


def os_props():
    return [
        enum("os", "Операционная система", "Operating system", OS_OPTIONS, group="platform", advanced=True,
             help=("Образ или базовая ОС: по ней проверяется срок поддержки.", "The image or base OS: its support dates are checked.")),
        prop("osVersion", "string", "Версия ОС", "OS version", group="platform", advanced=True, lifecycle="@os",
             help=("Например 24.04 или 9. Срок поддержки проверяется по выбранной ОС.", "For example 24.04 or 9. Support dates are checked for the chosen OS.")),
    ]


def runtime_props():
    return [
        enum("runtime", "Среда выполнения", "Runtime", RUNTIME_OPTIONS, group="platform", advanced=True,
             help=("Язык или платформа приложения: по ней проверяется срок поддержки.", "The language or platform of the application: its support dates are checked.")),
        prop("runtimeVersion", "string", "Версия среды выполнения", "Runtime version", group="platform", advanced=True, lifecycle="@runtime",
             help=("Например 22 для Node.js или 3.13 для Python.", "For example 22 for Node.js or 3.13 for Python.")),
    ]


# ---- new product kinds -------------------------------------------------------------------------------------------------
L7_TRAITS = ["balancer", "ingress", "edge"]
RB_LB = rb(failover=[("Проверьте, какой узел держит плавающий адрес, и что вторичный узел принял его после отказа.", "Check which node holds the floating address and that the standby took it over after the failure.")],
           verify=[("Запрос через балансировщик возвращает 200 с каждого из серверов пула.", "A request through the balancer returns 200 from every server of the pool.")])


def lb_kind(id, abbr, color, ru, en, table, cap, desc, variants, replicas=2, vcpu=2, ram=4, traits=None, fn="l7", extra=(), sub=None):
    props = [pv(table, default=rec(table) if table else None), pr(replicas)] + p_lb() + list(extra) + res(vcpu, ram)
    return k(id, "edge", fn, abbr, color, ru, en, "stateless", traits or L7_TRAITS, props, desc, sub=sub or "{replicas}× · {version}", cap=cap, variants=variants, runbook=RB_LB, versions=table if table and table != id else None)


KINDS_EDGE = [
    lb_kind("nginx", "NGX", "#bf1340", "NGINX", "NGINX", "nginx", 20000,
            ("Веб-сервер и обратный прокси: L7-балансировка, TLS, кэш. Отказоустойчивость — пара узлов с keepalived или несколько реплик в Kubernetes.",
             "A web server and reverse proxy: L7 balancing, TLS, a cache. Resilience comes from a keepalived pair or several replicas on Kubernetes."),
            [variant("vrrp", "На своих серверах (VRRP / keepalived)", "Own servers (VRRP / keepalived)", "Пара узлов с плавающим адресом.", "A pair of nodes with a floating address.", default=True, defaults={"hosting": "vm"}),
             variant("ingress", "Ingress-контроллер в Kubernetes", "Ingress controller on Kubernetes", "Реплики контроллера в кластере.", "Controller replicas on the cluster.", defaults={"hosting": "kubernetes"})]),
    lb_kind("haproxy", "HAP", "#bf1340", "HAProxy", "HAProxy", "haproxy", 30000,
            ("Быстрый балансировщик L4 и L7 с проверками состояния и плавным выводом узлов. Обычно работает парой с keepalived.",
             "A fast L4 and L7 balancer with health checks and graceful draining. Usually runs as a pair with keepalived."),
            [variant("vrrp", "На своих серверах (VRRP / keepalived)", "Own servers (VRRP / keepalived)", "Пара узлов с плавающим адресом.", "A pair of nodes with a floating address.", default=True, defaults={"hosting": "vm"}),
             variant("ingress", "Ingress-контроллер в Kubernetes", "Ingress controller on Kubernetes", "Реплики контроллера в кластере.", "Controller replicas on the cluster.", defaults={"hosting": "kubernetes"})]),
    lb_kind("envoy", "ENV", "#bf1340", "Envoy", "Envoy", "envoy", 15000,
            ("Прокси с динамической конфигурацией (xDS): граничный балансировщик, шлюз Gateway API или sidecar сервисной сетки.",
             "A proxy with dynamic configuration (xDS): an edge balancer, a Gateway API gateway or a service mesh sidecar."),
            [variant("edge", "Граничный прокси на виртуальных машинах", "Edge proxy on virtual machines", "Несколько реплик за балансировщиком L4.", "Several replicas behind an L4 balancer.", default=True, defaults={"hosting": "vm"}),
             variant("gateway", "Envoy Gateway (Gateway API в Kubernetes)", "Envoy Gateway (Gateway API on Kubernetes)", "Контроллер и прокси как ресурсы кластера.", "The controller and the proxies as cluster resources.", defaults={"hosting": "kubernetes"})]),
    lb_kind("traefik", "TRF", "#bf1340", "Traefik", "Traefik", "traefik", 10000,
            ("Обратный прокси с автоматическим обнаружением сервисов: Ingress и Gateway API в Kubernetes, Docker, Consul.",
             "A reverse proxy with automatic service discovery: Ingress and Gateway API on Kubernetes, Docker, Consul."),
            [variant("ingress", "Ingress-контроллер в Kubernetes", "Ingress controller on Kubernetes", "Реплики контроллера в кластере.", "Controller replicas on the cluster.", default=True, defaults={"hosting": "kubernetes"}),
             variant("standalone", "Отдельные узлы", "Standalone hosts", "Несколько узлов за балансировщиком L4.", "Several hosts behind an L4 balancer.", defaults={"hosting": "vm"})]),
    lb_kind("caddy", "CDY", "#bf1340", "Caddy", "Caddy", "caddy", 8000,
            ("Веб-сервер с автоматическим HTTPS. Для нескольких узлов сертификаты хранятся в общем хранилище.",
             "A web server with automatic HTTPS. With several nodes the certificates live in shared storage."), [], replicas=2),
    lb_kind("apache_httpd", "HTD", "#bf1340", "Apache HTTP Server", "Apache HTTP Server", "apache", 3000,
            ("Веб-сервер и обратный прокси с модулями mod_proxy и mod_security. Узлы ставят за общий балансировщик.",
             "A web server and reverse proxy with mod_proxy and mod_security. Nodes sit behind a shared balancer."), [], replicas=2, traits=["balancer", "edge"]),
    k("varnish", "edge", "http-cache", "VCL", "#2d6cdf", "Varnish Cache", "Varnish Cache", "stateless", ["proxy", "edge", "http-cache"],
      [pv("varnish"), pr(2)] + p_lb() + res(4, 16),
      ("Кэширующий HTTP-ускоритель перед приложением. Кэш на каждом узле свой, после перезапуска он пуст — приложение должно выдержать холодный старт.",
       "A caching HTTP accelerator in front of the application. Every node has its own cache and starts empty — the application must survive a cold start."),
      sub="{replicas}× · {version}", cap=80000, runbook=RB_LB),
    k("squid", "edge", "proxy", "SQD", "#2d6cdf", "Squid", "Squid", "stateless", ["proxy", "edge"],
      [pv("squid"), pr(2)] + res(2, 4, 50),
      ("Прокси-сервер для исходящего трафика и кэша. Узлы ставят за общий адрес, клиентам прописывают автообнаружение (WPAD/PAC).",
       "A proxy for outgoing traffic and caching. Nodes sit behind a shared address and clients get auto-discovery (WPAD/PAC)."),
      sub="{replicas}× · {version}", cap=5000),
    k("metallb", "edge", "l4", "MLB", "#2d6cdf", "MetalLB", "MetalLB", "stateless", ["balancer", "edge"],
      [pv("metallb"), pr(3, "Speaker-ы", "Speakers")] + res(0.5, 0.5),
      ("Балансировщик L4 для Kubernetes на своих серверах: выдаёт адреса сервисам типа LoadBalancer и объявляет их по ARP/NDP или BGP.",
       "An L4 balancer for bare-metal Kubernetes: gives addresses to LoadBalancer services and announces them over ARP/NDP or BGP."),
      sub="{replicas}× · {version}", cap=50000,
      variants=[variant("l2", "Режим L2 (ARP / NDP)", "L2 mode (ARP / NDP)", "Один узел отвечает за адрес; при его отказе другой подхватывает за секунды. Весь трафик адреса идёт через один узел.", "One node answers for an address; if it fails another takes over within seconds. All traffic for the address goes through one node.", default=True),
                variant("bgp", "Режим BGP (ECMP)", "BGP mode (ECMP)", "Каждый узел анонсирует адрес маршрутизатору, поток раскладывается на все узлы.", "Every node announces the address to the router, the flow is spread across all nodes.", defaults={"healthCheckSeconds": 3})]),
    k("firewall", "edge", "firewall", "FW", "#9d2235", "Межсетевой экран", "Firewall", "stateless", ["firewall", "edge"],
      [pv(None, "Версия ПО", "Software version"), pr(2, "Узлы кластера", "Cluster nodes")] + p_lb() + res(4, 8),
      ("Периметр и сегментация: пара узлов active/standby с синхронизацией состояния соединений. Одиночный экран — точка отказа всего периметра.",
       "Perimeter and segmentation: an active/standby pair with connection state sync. A single firewall is a single point of failure for the whole perimeter."),
      sub="{replicas}× · {version}", versions="",
      variants=[variant("opnsense", "OPNsense", "OPNsense", "Открытый межсетевой экран на FreeBSD; HA через CARP и pfsync.", "An open firewall on FreeBSD; HA through CARP and pfsync.", default=True, versions="opnsense"),
                variant("fortigate", "Fortinet FortiGate", "Fortinet FortiGate", "Кластер FGCP active-passive или active-active.", "An FGCP cluster, active-passive or active-active.", versions="fortigate"),
                variant("paloalto", "Palo Alto Networks (PAN-OS)", "Palo Alto Networks (PAN-OS)", "Пара HA active/passive с синхронизацией сессий.", "An HA pair, active/passive, with session sync.", versions="palo_alto"),
                variant("pfsense", "pfSense", "pfSense", "Открытый межсетевой экран; HA через CARP и pfsync.", "An open firewall; HA through CARP and pfsync.", versions=""),
                variant("cisco", "Cisco Secure Firewall (ASA / FTD)", "Cisco Secure Firewall (ASA / FTD)", "Пара failover с общим состоянием соединений.", "A failover pair with shared connection state.", versions=""),
                variant("cloud", "Облачный межсетевой экран (AWS Network Firewall, Azure Firewall)", "Cloud firewall (AWS Network Firewall, Azure Firewall)", "Провайдер держит узлы в нескольких зонах.", "The provider keeps nodes in several zones.", add_traits=["managed"], defaults={"hosting": "managed"}, versions="")]),
    k("network_device", "edge", "network", "NET", "#4c596c", "Маршрутизатор / коммутатор", "Router / switch", "stateless", ["network-device", "edge"],
      [pv(None, "Версия ОС устройства", "Device OS version"), pr(2, "Устройства в паре", "Devices in a pair")] + res(0, 0),
      ("Ядро, агрегация или spine/leaf площадки. Пара устройств (VSS, vPC, MLAG, EVPN-multihoming) переживает отказ одного; одиночное устройство — точка отказа сегмента.",
       "The core, aggregation or spine/leaf of a site. A pair (VSS, vPC, MLAG, EVPN multihoming) survives the loss of one device; a single device is a single point of failure for its segment."),
      sub="{replicas}× · {version}", versions="",
      variants=[variant("cisco", "Cisco IOS XE", "Cisco IOS XE", "Коммутаторы и маршрутизаторы Catalyst / ISR / ASR.", "Catalyst / ISR / ASR switches and routers.", default=True, versions="cisco_ios_xe"),
                variant("arista", "Arista EOS", "Arista EOS", "Коммутаторы центров данных, MLAG и EVPN.", "Data-centre switches, MLAG and EVPN.", versions=""),
                variant("juniper", "Juniper Junos", "Juniper Junos", "Маршрутизаторы и коммутаторы, Virtual Chassis и EVPN.", "Routers and switches, Virtual Chassis and EVPN.", versions=""),
                variant("sonic", "SONiC / Cumulus (open networking)", "SONiC / Cumulus (open networking)", "Открытая сетевая ОС на коммутаторах white-box.", "An open network OS on white-box switches.", versions="")]),
    k("dns_server", "edge", "dns-server", "NS", "#3a58c4", "DNS-сервер", "DNS server", "stateless", ["dns", "edge"],
      [pv(None, "Версия ПО", "Software version"), pr(2, "Серверы", "Servers")] + res(2, 4, 20),
      ("Авторитативный или рекурсивный DNS. Минимум два сервера в разных площадках; зону с одним первичным сервером можно обслуживать со вторичных, пока первичный недоступен.",
       "An authoritative or recursive DNS. At least two servers on different sites; a zone with one primary can be served from the secondaries while the primary is down."),
      sub="{replicas}× · {version}", cap=50000, versions="",
      variants=[variant("bind", "BIND 9", "BIND 9", "Эталонная реализация: первичный и вторичные серверы с передачей зон.", "The reference implementation: a primary and secondaries with zone transfer.", default=True, versions="bind"),
                variant("powerdns", "PowerDNS", "PowerDNS", "Авторитативный сервер с базой данных зон (SQL) и API; репликация зон средствами БД.", "An authoritative server with a zone database (SQL) and an API; zones replicate through the database.", versions=""),
                variant("coredns", "CoreDNS", "CoreDNS", "DNS на плагинах; штатный DNS кластера Kubernetes.", "Plugin-based DNS; the cluster DNS of Kubernetes.", versions=""),
                variant("cloud", "Облачная зона (Route 53, Cloud DNS…)", "Cloud zone (Route 53, Cloud DNS…)", "Серверы имён у провайдера на нескольких площадках.", "Name servers at the provider on several sites.", add_traits=["managed"], defaults={"hosting": "managed"}, versions="")]),
]

KINDS_COMPUTE = [
    k("nomad", "compute", "orchestrator", "NMD", "#2a64e0", "HashiCorp Nomad", "HashiCorp Nomad", "stateless", ["workload", "platform"],
      [pv("nomad"), pr(3, "Экземпляры задач", "Task instances"), num("hpaMin", "Автомасштабирование: минимум", "Autoscaling: minimum", min=1, group="scaling"), num("hpaMax", "Автомасштабирование: максимум", "Autoscaling: maximum", min=1, group="scaling")] + res(1, 2),
      ("Оркестратор для контейнеров, виртуальных машин и обычных процессов. Серверы объединены Raft (3 или 5), клиенты запускают задачи.",
       "An orchestrator for containers, virtual machines and plain processes. The servers form a Raft group (3 or 5), clients run the tasks."),
      sub="{replicas} tasks · HPA {hpaMin}–{hpaMax}", cap=150,
      runbook=rb(failover=[("Проверьте nomad server members: есть лидер и большинство серверов.", "Check nomad server members: there is a leader and a majority of servers."), ("Задачи с узлов отказавшей площадки перепланируются на живые клиенты — проверьте nomad job status.", "Tasks from the failed site are rescheduled onto live clients — check nomad job status.")])),
    k("kong", "compute", "api-gateway", "KNG", "#3a58c4", "Kong Gateway", "Kong Gateway", "stateless", ["gateway", "edge"],
      [pv("kong_gateway"), pr(3)] + res(2, 4),
      ("API-шлюз на основе NGINX: плагины аутентификации, лимитов, трансформаций. В режиме DB-less конфигурация приходит из Git, иначе нужна база PostgreSQL.",
       "An NGINX-based API gateway with authentication, rate-limiting and transformation plugins. In DB-less mode the configuration comes from Git, otherwise a PostgreSQL database is needed."),
      sub="{replicas}× · {version}", cap=8000, versions="kong_gateway",
      variants=[variant("dbless", "DB-less (конфигурация из Git)", "DB-less (configuration from Git)", "Каждая реплика читает декларативный файл; базы нет, отказывать нечему.", "Every replica reads a declarative file; there is no database to fail.", default=True),
                variant("postgres", "С базой данных PostgreSQL", "With a PostgreSQL database", "Конфигурация в общей базе: её недоступность не останавливает трафик, но блокирует изменения.", "The configuration is in a shared database: its outage does not stop the traffic but blocks changes."),
                variant("hybrid", "Hybrid (control plane + data plane)", "Hybrid (control plane + data plane)", "Узлы данных получают конфигурацию от управляющего узла и работают без него до перезапуска.", "Data-plane nodes get the configuration from a control plane and keep working without it until restart.")]),
    k("apisix", "compute", "api-gateway", "APX", "#3a58c4", "Apache APISIX", "Apache APISIX", "stateless", ["gateway", "edge"],
      [pv("apisix"), pr(3)] + res(2, 4),
      ("API-шлюз на NGINX/OpenResty с динамической конфигурацией. Правила хранятся в etcd; в автономном режиме — в файле.",
       "An API gateway on NGINX/OpenResty with dynamic configuration. Rules live in etcd; in standalone mode in a file."),
      sub="{replicas}× · {version}", cap=12000,
      variants=[variant("etcd", "Конфигурация в etcd", "Configuration in etcd", "Правила в кластере etcd; шлюз продолжает работать при его недоступности.", "Rules in an etcd cluster; the gateway keeps working when etcd is unreachable.", default=True),
                variant("standalone", "Автономный режим (файл)", "Standalone mode (file)", "Конфигурация из файла, без внешних зависимостей.", "Configuration from a file, no external dependencies.")]),
    k("airflow", "compute", "data-processing", "AFW", "#7a42d7", "Apache Airflow", "Apache Airflow", "stateless", ["workload", "processing"],
      [pv("airflow"), pr(2, "Scheduler и веб-серверы", "Schedulers and web servers")] + res(4, 8),
      ("Оркестратор пакетных заданий. Несколько планировщиков работают одновременно; метаданные лежат в PostgreSQL, очередь — в Redis или RabbitMQ при Celery.",
       "A batch workflow orchestrator. Several schedulers run at once; the metadata lives in PostgreSQL, the queue in Redis or RabbitMQ with Celery."),
      sub="{replicas}× · {version}", cap=50),
    k("spark", "compute", "data-processing", "SPK", "#7a42d7", "Apache Spark", "Apache Spark", "stateless", ["workload", "processing"],
      [pv("spark"), pr(4, "Исполнители", "Executors")] + res(8, 32, 200),
      ("Распределённая обработка данных. Мастер в режиме высокой доступности — через ZooKeeper или Kubernetes; потеря исполнителя ведёт к повтору задач.",
       "Distributed data processing. A master in high-availability mode runs through ZooKeeper or Kubernetes; losing an executor means tasks are retried."),
      sub="{replicas}× · {version}", cap=20),
    k("flink", "compute", "data-processing", "FLK", "#7a42d7", "Apache Flink", "Apache Flink", "stateless", ["workload", "processing"],
      [pv("flink"), pr(3, "TaskManager-ы", "TaskManagers")] + res(4, 16, 100),
      ("Потоковая обработка с состоянием. JobManager в режиме высокой доступности требует ZooKeeper или Kubernetes; состояние восстанавливается из чекпоинтов в объектном хранилище.",
       "Stateful stream processing. A highly available JobManager needs ZooKeeper or Kubernetes; the state is restored from checkpoints in object storage."),
      sub="{replicas}× · {version}", cap=100000),
    k("nifi", "compute", "data-processing", "NIF", "#7a42d7", "Apache NiFi", "Apache NiFi", "stateless", ["workload", "processing"],
      [pv("nifi"), pr(3, "Узлы кластера", "Cluster nodes")] + res(4, 16, 200),
      ("Потоки данных между системами с визуальным конструктором. Кластер на ZooKeeper (или Kubernetes), каждый узел обрабатывает свою долю.",
       "Data flows between systems with a visual designer. A cluster on ZooKeeper (or Kubernetes), every node handles its share."),
      sub="{replicas}× · {version}", cap=5000),
]
