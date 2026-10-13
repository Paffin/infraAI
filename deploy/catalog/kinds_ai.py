"""Category `ai`: ML platforms and MLOps, model serving, LLM runtimes and gateways, notebooks, feature stores, GPU and ML scheduling, data labeling.

Inference servers and LLM gateways sit on the request path (`app`, with `cap` in requests per second per instance: GPU memory and batch size decide it,
not the CPU). Training and MLOps platforms, notebooks and feature stores are control plane or stateful tooling (`platform`, no `cap`).
"""
from forge import *  # noqa: F401,F403

FUNCTIONS = {"ai": [
    ("ml-platform", "Платформы ML и MLOps", "ML platforms and MLOps", "flask-conical"),
    ("model-serving", "Обслуживание моделей", "Model serving", "brain-circuit"),
    ("llm-runtime", "Запуск LLM и шлюзы", "LLM runtimes and gateways", "sparkles"),
    ("notebooks", "Ноутбуки и среды разработки", "Notebooks and workbenches", "notebook-pen"),
    ("feature-store", "Feature store", "Feature stores", "table-2"),
    ("ml-infra", "GPU и планирование для ML", "GPU and ML scheduling", "microchip"),
    ("data-labeling", "Разметка данных", "Data labeling", "tags"),
]}
IAC = {"ml-platform": "ml", "model-serving": "ml", "llm-runtime": "ml", "notebooks": "ml", "feature-store": "ml", "ml-infra": "ml", "data-labeling": "ml"}

SERVE = T["app"] + ["ml"]                                    # on the request path: an application tier with a capacity model
PLATFORM = T["platform"] + ["ml"]                            # control plane / tooling: no capacity model
PLATFORM_STATE = PLATFORM + ["stateful", "backup-required"]  # tooling whose own disks hold data that must be backed up
GPU = num("gpus", "GPU на экземпляр", "GPUs per instance", min=0, default=1, group="capacity")
SUB_GPU = "{replicas}× · {gpus} GPU · {version}"

KINDS = [
    # ---- ML platforms and MLOps -------------------------------------------------------------------------------------------------------
    svc("kubeflow", "ai", "ml-platform", "KF", "Kubeflow", "Kubeflow",
        ("Набор компонентов ML для Kubernetes: Pipelines, Notebooks, Trainer, Katib и Hub (реестр моделей); Pipelines по умолчанию хранит метаданные в MySQL, а артефакты — в SeaweedFS (до KFP 2.15 — в MinIO). "
         "В стандартных манифестах база работает одним подом на томе внутри кластера, поэтому для рабочей среды базу и объектное хранилище выносят за пределы кластера или резервируют; платформа доступна не лучше самого кластера Kubernetes.",
         "A set of ML components for Kubernetes: Pipelines, Notebooks, Trainer, Katib and the Hub model registry; Pipelines keeps its metadata in MySQL and its artifacts in SeaweedFS by default (MinIO before KFP 2.15). "
         "In the standard manifests the database is a single pod on a volume inside the cluster, so for production the database and object storage move out of the cluster or are made redundant; the platform is no more available than its Kubernetes cluster."),
        traits=PLATFORM_STATE, replicas=2, rep=("Экземпляры компонентов", "Component replicas"), vcpu=4, ram=16, disk=200),
    svc("mlflow", "ai", "ml-platform", "MLF", "MLflow", "MLflow",
        ("Учёт экспериментов и реестр моделей: серверы отслеживания сами состояния не хранят — метаданные лежат в SQL-базе (PostgreSQL, MySQL), артефакты — в объектном хранилище. "
         "Несколько серверов за балансировщиком переживают потерю любого из них; потеря базы или хранилища артефактов останавливает запись экспериментов и доступ к моделям.",
         "Experiment tracking and a model registry: tracking servers keep no state of their own — metadata goes to a SQL database (PostgreSQL, MySQL) and artifacts to object storage. "
         "Several servers behind a balancer survive the loss of any one; losing the database or the artifact store stops experiment logging and access to models."),
        traits=PLATFORM, replicas=2, rep=("Серверы отслеживания", "Tracking servers"), vcpu=2, ram=4,
        runbook=rb(restore=[("Восстановите базу метаданных и хранилище артефактов из копий на один момент времени: иначе запуски в базе ссылаются на отсутствующие артефакты.",
                             "Restore the metadata database and the artifact store from backups of the same moment: otherwise the runs in the database point to missing artifacts.")],
                   verify=[("Последний запуск открывается в интерфейсе вместе с параметрами, метриками и артефактами, а модель из реестра загружается.",
                            "The latest run opens in the UI with its parameters, metrics and artifacts, and a model from the registry loads.")]),
        variants=[vself("self-hosted", "Свои серверы отслеживания", "Own tracking servers",
                        "Серверы на ваших машинах или в Kubernetes; база метаданных и хранилище артефактов — отдельные узлы, их нужно резервировать и копировать.",
                        "Servers on your machines or on Kubernetes; the metadata database and the artifact store are separate nodes that need redundancy and backups.", default=True),
                  vcloud("managed", "Управляемый MLflow (Databricks, Amazon SageMaker AI)", "Managed MLflow (Databricks, Amazon SageMaker AI)", secs=None, mult=1.5)]),
    svc("clearml", "ai", "ml-platform", "CML", "ClearML", "ClearML",
        ("Платформа MLOps: учёт экспериментов, версий данных и моделей, очереди заданий, которые выполняют агенты. "
         "Сервер (API, веб-интерфейс, файловый сервер) хранит данные в MongoDB, Elasticsearch и Redis, а файлы — на своём томе; в поставке по умолчанию каждое хранилище — один контейнер, поэтому для высокой доступности их кластеризуют или выносят.",
         "An MLOps platform: tracking of experiments, dataset versions and models, and job queues served by agents. "
         "The server (API, web UI, file server) keeps its data in MongoDB, Elasticsearch and Redis and its files on its own volume; by default each store is a single container, so for high availability they are clustered or moved out."),
        traits=PLATFORM_STATE, replicas=1, rep=("Экземпляры сервера", "Server instances"), vcpu=4, ram=16, disk=500,
        variants=[vself("self-hosted", "Свой сервер ClearML", "Own ClearML server",
                        "Сервер на ваших машинах или в Kubernetes (Helm); защита баз и файлов — на вас.",
                        "The server on your machines or on Kubernetes (Helm); protecting the databases and files is up to you.", default=True),
                  vcloud("hosted", "ClearML Hosted Service", "ClearML Hosted Service", secs=None, mult=1.5)]),
    svc("langfuse", "ai", "ml-platform", "LFU", "Langfuse", "Langfuse",
        ("Платформа наблюдаемости для LLM-приложений: трассы, оценки и промпты; данные лежат в PostgreSQL (транзакционные), ClickHouse (трассы и оценки), Redis (очередь и кэш) и S3 (сырые события), а веб-приложение и воркер их только обрабатывают. "
         "Веб-приложение сначала пишет события в S3 и ставит их в очередь Redis, поэтому при кратковременной недоступности базы они не теряются; Docker Compose — одна ВМ без высокой доступности, масштабирования и резервных копий.",
         "An observability platform for LLM applications: traces, evaluations and prompts; data lives in PostgreSQL (transactional), ClickHouse (traces and scores), Redis (queue and cache) and S3 (raw events), while the web application and the worker only process it. "
         "The web application first writes events to S3 and queues them in Redis, so they are not lost when the database is briefly unavailable; Docker Compose is a single VM without high availability, scaling or backups."),
        traits=PLATFORM, replicas=2, rep=("Реплики web и worker", "Web and worker replicas"), vcpu=4, ram=8,
        variants=[vself("kubernetes", "Helm-чарт в Kubernetes или облаке", "Helm chart on Kubernetes or in a cloud",
                        "Веб-приложение и воркер работают репликами; PostgreSQL, ClickHouse, Redis и S3 — отдельные узлы, их отказоустойчивость на вас.",
                        "The web application and the worker run as replicas; PostgreSQL, ClickHouse, Redis and S3 are separate nodes whose resilience is up to you.", default=True),
                  vself("docker-compose", "Docker Compose (одна ВМ)", "Docker Compose (a single VM)",
                        "Все службы на одной ВМ — без высокой доступности, масштабирования и резервных копий.",
                        "All services on one VM — with no high availability, scaling or backups.", defaults={"replicas": 1, "hosting": "vm"}),
                  vcloud("cloud", "Langfuse Cloud", "Langfuse Cloud", secs=None, mult=1.5)]),
    svc("red_hat_openshift_ai", "ai", "ml-platform", "RHAI", "Red Hat OpenShift AI", "Red Hat OpenShift AI",
        ("Платформа Red Hat для жизненного цикла моделей на кластере OpenShift: среды разработки, конвейеры, обслуживание моделей (в том числе на vLLM) и каталог моделей. "
         "Это компоненты внутри уже существующего кластера OpenShift, поэтому платформа доступна не лучше самого кластера; модели и артефакты хранятся в подключённом объектном хранилище.",
         "A Red Hat platform for the model lifecycle on an OpenShift cluster: workbenches, pipelines, model serving (vLLM included) and a model catalog. "
         "The components run inside an existing OpenShift cluster, so the platform is no more available than the cluster itself; models and artifacts live in attached object storage."),
        traits=PLATFORM_STATE, replicas=2, rep=("Экземпляры компонентов", "Component replicas"), vcpu=4, ram=16, disk=200),
    saas("amazon_sagemaker", "ai", "ml-platform", "SGM", "Amazon SageMaker AI", "Amazon SageMaker AI",
         ("Управляемая платформа AWS для обучения, дообучения и размещения моделей: ноутбуки, задания обучения, конвейеры, реестр моделей и конечные точки инференса. "
          "Управляющий слой ведёт AWS; для отказоустойчивой конечной точки задают два и более экземпляров — SageMaker старается раскладывать их по зонам доступности.",
          "The AWS managed platform for training, fine-tuning and hosting models: notebooks, training jobs, pipelines, a model registry and inference endpoints. "
          "AWS runs the control plane; for a resilient endpoint you set two or more instances, which SageMaker tries to spread across Availability Zones."),
         traits=PLATFORM),
    saas("google_vertex_ai", "ai", "ml-platform", "VTX", "Google Vertex AI (Gemini Enterprise Agent Platform)", "Google Vertex AI (Gemini Enterprise Agent Platform)",
         ("Управляемая платформа Google Cloud для обучения, дообучения и размещения моделей; с апреля 2026 года Vertex AI входит в Gemini Enterprise Agent Platform. "
          "Управляющий слой и конечные точки ведёт Google; конечная точка региональная, а её устойчивость задаётся минимальным числом реплик и автомасштабированием.",
          "The Google Cloud managed platform for training, fine-tuning and serving models; since April 2026 Vertex AI is part of the Gemini Enterprise Agent Platform. "
          "Google runs the control plane and the endpoints; an endpoint is regional, and its resilience comes from the minimum replica count and autoscaling."),
         traits=PLATFORM),
    saas("azure_machine_learning", "ai", "ml-platform", "AML", "Azure Machine Learning", "Azure Machine Learning",
         ("Управляемая платформа Microsoft Azure для обучения и размещения моделей: рабочие пространства, вычислительные кластеры, конвейеры, реестр моделей и управляемые онлайн-конечные точки. "
          "Управляющий слой ведёт Azure; для высокой доступности развёртыванию конечной точки задают не менее трёх экземпляров, а регион выбираете вы.",
          "The Microsoft Azure managed platform for training and hosting models: workspaces, compute clusters, pipelines, a model registry and managed online endpoints. "
          "Azure runs the control plane; for high availability give an endpoint deployment at least three instances, and the region is your choice."),
         traits=PLATFORM),
    saas("yandex_datasphere", "ai", "ml-platform", "YDS", "Yandex DataSphere", "Yandex DataSphere",
         ("Управляемая среда разработки ML в Yandex Cloud: ноутбуки с JupyterLab, запуск заданий и размещение моделей (DataSphere Inference). "
          "Сервис целиком ведёт Yandex Cloud; ресурсы выделяются на время работы, а ВМ ноутбука освобождается после простоя (по умолчанию через три часа).",
          "A managed ML development environment in Yandex Cloud: JupyterLab notebooks, remote jobs and model hosting (DataSphere Inference). "
          "Yandex Cloud runs the whole service; resources are allocated while you work and a notebook VM is released after idling (three hours by default)."),
         traits=PLATFORM),
    saas("hugging_face_hub", "ai", "ml-platform", "HF", "Hugging Face Hub", "Hugging Face Hub",
         ("Облачный каталог моделей и датасетов Hugging Face: репозитории с весами, из которых серверы инференса вроде vLLM и SGLang по умолчанию скачивают модели при старте. "
          "Сервис ведёт Hugging Face, но экземпляры, которые стартуют во время его сбоя, не получат веса, поэтому для рабочей среды веса копируют в своё объектное хранилище или на общий том.",
          "The Hugging Face cloud catalog of models and datasets: repositories of weights from which inference servers such as vLLM and SGLang download models at start by default. "
          "Hugging Face runs the service, but instances that start during its outage get no weights, so for production the weights are copied to your own object storage or a shared volume."),
         traits=PLATFORM),

    # ---- Model serving ------------------------------------------------------------------------------------------------------------------
    app("kserve", "ai", "model-serving", "KSV", "KServe", "KServe",
        ("Платформа обслуживания моделей в Kubernetes: ресурс InferenceService запускает предсказатели для классических моделей и LLM (vLLM) и строит графы инференса. "
         "Контроллер работает отдельно от предсказателей — его потеря блокирует изменения, но запущенные модели отвечают; пропускную способность реплики определяют память GPU и размер батча, а не процессор.",
         "A model serving platform for Kubernetes: an InferenceService runs predictors for classical models and LLMs (vLLM) and builds inference graphs. "
         "The controller runs apart from the predictors — losing it blocks changes while running models keep answering; the throughput of a replica is decided by GPU memory and batch size, not by the CPU."),
        cap=100, traits=SERVE, replicas=2, rep=("Реплики предсказателя", "Predictor replicas"), extra=[GPU], vcpu=4, ram=16, sub=SUB_GPU,
        variants=[vself("raw", "Обычные Deployment (RawDeployment)", "Plain Deployments (RawDeployment)",
                        "Предсказатели — обычные Deployment с автомасштабированием HPA, без Knative; нет масштабирования до нуля по запросам и канареечных выкладок.",
                        "Predictors are plain Deployments autoscaled by HPA, with no Knative; there is no request-based scale to zero and no canary rollouts.",
                        default=True, defaults={"hosting": "kubernetes"}),
                  vself("serverless", "Knative (serverless)", "Knative (serverless)",
                        "Knative масштабирует предсказатели по числу запросов, в том числе до нуля, и делит трафик для канареечных выкладок; зависит от Knative и сетевого слоя, а запрос к «спящей» модели ждёт холодного старта.",
                        "Knative scales predictors by request count, down to zero included, and splits traffic for canary rollouts; it depends on Knative and its network layer, and a request to a sleeping model waits for a cold start.",
                        defaults={"hosting": "kubernetes"})]),
    app("seldon_core", "ai", "model-serving", "SLD", "Seldon Core", "Seldon Core",
        ("Платформа развёртывания моделей и LLM-приложений в Kubernetes (Seldon Core 2): общие серверы инференса, конвейеры из нескольких моделей, эксперименты и теневые развёртывания. "
         "Конвейеры передают данные между шагами через Kafka, поэтому её потеря останавливает конвейеры; пропускную способность реплики определяют память GPU и размер батча, а не процессор.",
         "A platform for deploying models and LLM applications on Kubernetes (Seldon Core 2): shared inference servers, multi-model pipelines, experiments and shadow deployments. "
         "Pipelines pass data between steps through Kafka, so losing it stops the pipelines; the throughput of a replica is decided by GPU memory and batch size, not by the CPU."),
        cap=100, traits=SERVE, replicas=2, rep=("Реплики сервера инференса", "Inference server replicas"), extra=[GPU], vcpu=4, ram=16, sub=SUB_GPU),
    app("bentoml", "ai", "model-serving", "BNT", "BentoML", "BentoML",
        ("Фреймворк и среда выполнения сервисов инференса на Python: модель и логика упаковываются в Bento и контейнер, а адаптивный батчинг объединяет запросы. "
         "Каждый экземпляр — независимый контейнер без общего состояния, поэтому отказ одного не ломает остальных; пропускную способность определяют память GPU и размер батча, а не процессор.",
         "A Python framework and runtime for inference services: the model and its logic are packaged as a Bento and a container, and adaptive batching groups requests. "
         "Each instance is an independent container with no shared state, so losing one does not break the others; throughput is decided by GPU memory and batch size, not by the CPU."),
        cap=100, traits=SERVE, replicas=2, extra=[GPU], vcpu=4, ram=16, sub=SUB_GPU,
        variants=[vself("self-hosted", "Свои контейнеры", "Own containers",
                        "Контейнеры Bento за балансировщиком или Service в Kubernetes; масштабирование и обновления — на вас.",
                        "Bento containers behind a balancer or a Kubernetes Service; scaling and upgrades are up to you.", default=True),
                  vcloud("cloud", "BentoCloud", "BentoCloud", secs=None, mult=1.5)]),
    app("nvidia_triton", "ai", "model-serving", "TIS", "NVIDIA Dynamo-Triton (Triton Inference Server)", "NVIDIA Dynamo-Triton (Triton Inference Server)",
        ("Сервер инференса NVIDIA (до марта 2025 года — Triton Inference Server): бэкенды TensorRT, PyTorch, ONNX, OpenVINO и другие, модели из репозитория на файловой системе, динамический батчинг и параллельное исполнение моделей. "
         "Экземпляры независимы — отказ одного снимает его долю ёмкости; пропускную способность определяют память GPU и размер батча, а не процессор.",
         "The NVIDIA inference server (formerly Triton Inference Server, now part of the NVIDIA Dynamo platform): TensorRT, PyTorch, ONNX, OpenVINO and other backends, models from a file-system repository, dynamic batching and concurrent model execution. "
         "Instances are independent — losing one removes its share of capacity; throughput is decided by GPU memory and batch size, not by the CPU."),
        cap=200, traits=SERVE, replicas=2, extra=[GPU], vcpu=8, ram=32, sub=SUB_GPU),
    app("torchserve", "ai", "model-serving", "TS", "TorchServe", "TorchServe",
        ("Сервер моделей PyTorch с обработчиками на Python, REST и gRPC и управлением версиями моделей. "
         "Владелец архивировал проект в августе 2025 года, исправления (в том числе безопасности) больше не выходят — для новых систем выберите другой сервер; экземпляры независимы, пропускную способность определяют память GPU и размер батча.",
         "A PyTorch model server with Python handlers, REST and gRPC APIs and model version management. "
         "The owner archived the project in August 2025 and no fixes, security fixes included, are released any more, so pick another server for new systems; instances are independent and throughput is decided by GPU memory and batch size."),
        cap=50, traits=SERVE, replicas=2, extra=[GPU], vcpu=4, ram=16, sub=SUB_GPU),
    app("ray_serve", "ai", "model-serving", "RSV", "Ray Serve", "Ray Serve",
        ("Библиотека обслуживания моделей на кластере Ray: развёртывания с автомасштабированием и композиция моделей на Python; пропускную способность реплики определяют память GPU и размер батча, а не процессор. "
         "Упавшую реплику Serve перезапускает сам, потерянный рабочий узел заменяет KubeRay (на Kubernetes), а головной узел без отказоустойчивости GCS (внешний Redis) — единая точка отказа всего кластера; с ней рабочие узлы продолжают отвечать, пока голова восстанавливается.",
         "A model serving library on a Ray cluster: autoscaling deployments and Python model composition; the throughput of a replica is decided by GPU memory and batch size, not by the CPU. "
         "Serve restarts a failed replica itself and KubeRay (on Kubernetes) replaces a lost worker node, while the head node without GCS fault tolerance (an external Redis) is a single point of failure for the whole cluster; with it, worker nodes keep serving while the head recovers."),
        cap=100, traits=SERVE, replicas=2, rep=("Реплики развёртывания", "Deployment replicas"), extra=[GPU], vcpu=8, ram=32, sub=SUB_GPU,
        runbook=rb(failover=[("serve status показывает приложения в состоянии RUNNING и реплики HEALTHY; на Kubernetes проверьте также kubectl describe rayservice и что головной под перезапущен.",
                              "serve status shows the applications RUNNING and the replicas HEALTHY; on Kubernetes also check kubectl describe rayservice and that the head pod has restarted."),
                             ("Без отказоустойчивости GCS потеря головного узла перезапускает весь кластер — дождитесь, пока рабочие поды и развёртывания инициализируются заново.",
                              "Without GCS fault tolerance losing the head node restarts the whole cluster — wait for the worker pods and the deployments to initialise again.")])),
    app("tensorflow_serving", "ai", "model-serving", "TFS", "TensorFlow Serving", "TensorFlow Serving",
        ("Высокопроизводительный сервер моделей TensorFlow (gRPC и REST): несколько версий модели одновременно, выкладка новых версий без изменения клиентов. "
         "Экземпляры независимы и общего состояния не имеют — отказ одного снимает его долю ёмкости; пропускную способность определяют память GPU и размер батча, а не процессор.",
         "A high-performance TensorFlow model server (gRPC and REST) that serves several versions of a model at once and rolls out new versions without changing clients. "
         "Instances are independent and hold no shared state, so losing one removes its share of capacity; throughput is decided by GPU memory and batch size, not by the CPU."),
        cap=200, traits=SERVE, replicas=2, extra=[GPU], vcpu=4, ram=16, sub=SUB_GPU),
    app("openvino_model_server", "ai", "model-serving", "OVMS", "OpenVINO Model Server", "OpenVINO Model Server",
        ("Сервер Intel для моделей OpenVINO на CPU, графических ускорителях Intel и NPU: API KServe и OpenAI-совместимое API для текста, эмбеддингов, изображений и аудио, модели из локального хранилища, S3, GCS, Azure Blob или Hugging Face. "
         "Экземпляры независимы — отказ одного снимает его долю ёмкости; пропускную способность определяют ядра, число потоков и размер батча.",
         "An Intel server for OpenVINO models on CPUs, Intel graphics accelerators and NPUs: KServe APIs plus an OpenAI-compatible API for text, embeddings, images and audio, with models from local storage, S3, GCS, Azure Blob or Hugging Face. "
         "Instances are independent — losing one removes its share of capacity; throughput is decided by cores, threads and batch size."),
        cap=200, traits=SERVE, replicas=2, extra=[{**GPU, "default": 0}], vcpu=8, ram=16, sub=SUB_GPU),

    # ---- LLM runtimes and gateways ------------------------------------------------------------------------------------------------------
    app("vllm", "ai", "llm-runtime", "VLLM", "vLLM", "vLLM",
        ("Движок и сервер инференса LLM с OpenAI-совместимым API: непрерывный батчинг, PagedAttention, тензорный и конвейерный параллелизм. "
         "Экземпляр держит модель в памяти GPU: при его отказе текущие запросы обрываются, а замена загружает веса минуты, поэтому держите запас реплик; пропускную способность определяют память GPU и размер батча, а не процессор.",
         "An LLM inference engine and server with an OpenAI-compatible API: continuous batching, PagedAttention, tensor and pipeline parallelism. "
         "An instance holds the model in GPU memory: when it fails its in-flight requests are cut and a replacement needs minutes to load the weights, so keep spare replicas; throughput is decided by GPU memory and batch size, not by the CPU."),
        cap=10, traits=SERVE, replicas=2, extra=[GPU], vcpu=8, ram=64, disk=200, sub=SUB_GPU,
        runbook=rb(failover=[("Новый экземпляр включайте в балансировщик, только когда /health вернул 200: до этого веса ещё загружаются, и запросы получат ошибку.",
                              "Add a new instance to the balancer only once /health returns 200: until then the weights are still loading and requests would fail.")],
                   verify=[("GET /v1/models на каждом экземпляре возвращает ожидаемую модель, а пробный запрос к /v1/chat/completions отвечает.",
                            "GET /v1/models on every instance returns the expected model and a test request to /v1/chat/completions is answered.")])),
    app("sglang", "ai", "llm-runtime", "SGL", "SGLang", "SGLang",
        ("Фреймворк инференса LLM, мультимодальных и диффузионных моделей с сервером, совместимым с OpenAI API; работает на ускорителях NVIDIA, AMD, Intel и других. "
         "Экземпляр держит модель в памяти GPU: его отказ обрывает текущие запросы, а замена загружает веса минуты, поэтому держите запас реплик; пропускную способность определяют память GPU и размер батча, а не процессор.",
         "An inference framework for LLMs, multimodal and diffusion models with an OpenAI-compatible server; it runs on NVIDIA, AMD, Intel and other accelerators. "
         "An instance holds the model in GPU memory: its failure cuts in-flight requests and a replacement needs minutes to load the weights, so keep spare replicas; throughput is decided by GPU memory and batch size, not by the CPU."),
        cap=10, traits=SERVE, replicas=2, extra=[GPU], vcpu=8, ram=64, disk=200, sub=SUB_GPU),
    app("text_generation_inference", "ai", "llm-runtime", "TGI", "Hugging Face Text Generation Inference", "Hugging Face Text Generation Inference",
        ("Сервер инференса LLM от Hugging Face с непрерывным батчингом и потоковой выдачей токенов. "
         "Проект в режиме сопровождения, репозиторий архивирован в марте 2026 года, а авторы рекомендуют vLLM и SGLang — для новых систем выбирайте их; экземпляры независимы, пропускную способность определяют память GPU и размер батча, а не процессор.",
         "The Hugging Face LLM inference server with continuous batching and token streaming. "
         "The project is in maintenance mode, its repository was archived in March 2026 and its authors recommend vLLM and SGLang, so choose those for new systems; instances are independent and throughput is decided by GPU memory and batch size, not by the CPU."),
        cap=10, traits=SERVE, replicas=2, extra=[GPU], vcpu=8, ram=64, disk=200, sub=SUB_GPU),
    app("ollama", "ai", "llm-runtime", "OLL", "Ollama", "Ollama",
        ("Лёгкий сервер для запуска открытых LLM на одном узле: подгружает модели в память по запросу и отдаёт HTTP API, в том числе OpenAI-совместимое. "
         "Встроенной кластеризации нет, экземпляры независимы; запросы сверх OLLAMA_NUM_PARALLEL (по умолчанию 1) ждут в очереди до 512, дальше сервер отвечает 503; пропускную способность определяют память GPU и параллелизм, а не процессор.",
         "A light server for running open LLMs on a single node: it loads models into memory on demand and exposes an HTTP API, OpenAI-compatible included. "
         "There is no built-in clustering, so instances are independent; requests beyond OLLAMA_NUM_PARALLEL (1 by default) wait in a queue of up to 512 and then get a 503; throughput is decided by GPU memory and parallelism, not by the CPU."),
        cap=1, traits=SERVE, replicas=2, extra=[GPU], vcpu=8, ram=32, disk=100, sub=SUB_GPU),
    app("llama_cpp_server", "ai", "llm-runtime", "LCPP", "llama.cpp server", "llama.cpp server",
        ("HTTP-сервер движка llama.cpp: OpenAI-совместимое API, параллельная обработка запросов через слоты и непрерывный батчинг; работает на CPU и на GPU разных производителей. "
         "Кластеризации нет: каждый экземпляр держит модель и кэш промптов у себя, а /health отвечает 503, пока модель загружается, поэтому трафик пускают только после готовности; пропускную способность определяют число слотов, память и размер батча.",
         "The HTTP server of the llama.cpp engine: an OpenAI-compatible API, parallel requests through slots and continuous batching; it runs on CPUs and on GPUs of several vendors. "
         "There is no clustering: each instance keeps its own model and prompt cache, and /health answers 503 while the model loads, so route traffic only once it is ready; throughput is decided by slots, memory and batch size."),
        cap=2, traits=SERVE, replicas=2, extra=[{**GPU, "default": 0}], vcpu=8, ram=32, disk=50, sub=SUB_GPU,
        runbook=rb(failover=[("Новый экземпляр включайте в балансировщик, когда /health вернул 200 и status ok: пока модель загружается, он отвечает 503.",
                              "Add a new instance to the balancer once /health returns 200 with status ok: while the model loads it answers 503.")])),
    app("localai", "ai", "llm-runtime", "LAI", "LocalAI", "LocalAI",
        ("Локальный сервер с API, совместимым с OpenAI и Anthropic, для LLM, изображений и аудио: десятки бэкендов (llama.cpp, vLLM, whisper.cpp, diffusers), работает на CPU и на GPU, GPU не обязателен. "
         "Экземпляр — один узел со своими моделями; распределённый режим масштабируется горизонтально с PostgreSQL и NATS; пропускную способность определяют память, ядра и размер батча.",
         "A self-hosted server with OpenAI- and Anthropic-compatible APIs for LLMs, images and audio: dozens of backends (llama.cpp, vLLM, whisper.cpp, diffusers); it runs on CPUs or GPUs and needs no GPU. "
         "An instance is one node with its own models; a distributed mode scales horizontally with PostgreSQL and NATS; throughput is decided by memory, cores and batch size."),
        cap=2, traits=SERVE, replicas=2, extra=[{**GPU, "default": 0}], vcpu=8, ram=32, disk=100, sub=SUB_GPU),
    app("nvidia_nim", "ai", "llm-runtime", "NIM", "NVIDIA NIM", "NVIDIA NIM",
        ("Контейнерные микросервисы NVIDIA с готовым оптимизированным движком инференса и OpenAI-совместимым API: экземпляр обслуживает одну модель на GPU NVIDIA. "
         "Без общего кэша каждый экземпляр скачивает свою копию модели при старте (NIM Operator держит общий кэш на сетевом хранилище), поэтому замена занимает минуты; пропускную способность определяют память GPU и размер батча, а не процессор.",
         "NVIDIA container microservices with a ready optimised inference engine and an OpenAI-compatible API: an instance serves one model on NVIDIA GPUs. "
         "Without a shared cache each instance downloads its own copy of the model at start (the NIM Operator keeps a shared cache on network storage), so a replacement takes minutes; throughput is decided by GPU memory and batch size, not by the CPU."),
        cap=10, traits=SERVE, replicas=2, extra=[GPU], vcpu=8, ram=64, disk=200, sub=SUB_GPU),
    app("nvidia_dynamo", "ai", "llm-runtime", "DYN", "NVIDIA Dynamo", "NVIDIA Dynamo",
        ("Открытый стек распределённого инференса NVIDIA над движками vLLM, SGLang и TensorRT-LLM: OpenAI-совместимый фронтенд, маршрутизатор с учётом нагрузки и KV-кэша, отдельные пулы prefill и decode, планировщик масштабирования по SLA. "
         "Проект заявляет проверки состояния и перенос запросов в полёте при отказе рабочего узла; пропускную способность определяют память GPU и размер батча, а не процессор.",
         "The NVIDIA open-source distributed inference stack above the vLLM, SGLang and TensorRT-LLM engines: an OpenAI-compatible frontend, a router that accounts for load and KV-cache overlap, separate prefill and decode worker pools and an SLA-driven planner. "
         "The project documents health checks and migration of in-flight requests when a worker fails; throughput is decided by GPU memory and batch size, not by the CPU."),
        cap=10, traits=SERVE, replicas=2, rep=("Рабочие узлы", "Workers"), extra=[GPU], vcpu=16, ram=128, disk=200, sub=SUB_GPU),
    app("litellm", "ai", "llm-runtime", "LTL", "LiteLLM", "LiteLLM",
        ("Шлюз к LLM (LiteLLM Proxy): единый OpenAI-совместимый API ко многим провайдерам и своим моделям, виртуальные ключи, бюджеты, лимиты и переключение между развёртываниями; ёмкость экземпляра оценена по накладным расходам самого шлюза, без времени ответа модели. "
         "Экземпляры без состояния, но ключи и расходы лежат в PostgreSQL, а общие счётчики лимитов и кэш — в Redis: без него при нескольких экземплярах каждый считает лимиты сам.",
         "An LLM gateway (LiteLLM Proxy): one OpenAI-compatible API to many providers and your own models, with virtual keys, budgets, rate limits and failover between deployments; the capacity of an instance is estimated from the overhead of the gateway itself, not from the model's response time. "
         "Instances are stateless, but keys and spend live in PostgreSQL and shared rate-limit counters and the cache in Redis: without it, with several instances, each enforces limits on its own."),
        cap=300, traits=SERVE, replicas=2, vcpu=4, ram=8),
    app("open_webui", "ai", "llm-runtime", "OWU", "Open WebUI", "Open WebUI",
        ("Веб-интерфейс для чатов с LLM (Ollama и OpenAI-совместимые API): пользователи, история, документы и RAG. "
         "Один контейнер с SQLite и локальной векторной базой — единая точка отказа; для нескольких реплик нужны PostgreSQL, Redis для сессий и WebSocket, общее хранилище файлов или S3, внешняя векторная база и общий WEBUI_SECRET_KEY.",
         "A web interface for chatting with LLMs (Ollama and OpenAI-compatible APIs): users, history, documents and RAG. "
         "A single container with SQLite and a local vector store is a single point of failure; several replicas need PostgreSQL, Redis for sessions and WebSockets, shared file storage or S3, an external vector database and a shared WEBUI_SECRET_KEY."),
        cap=50, traits=SERVE, replicas=2, vcpu=2, ram=4, disk=50,
        variants=[vself("single", "Один контейнер (SQLite)", "Single container (SQLite)",
                        "Данные, загруженные файлы и векторная база лежат на томе контейнера: его отказ останавливает интерфейс, том нужно копировать.",
                        "Data, uploaded files and the vector store live on the container's volume: its failure stops the interface, and the volume needs backups.",
                        default=True, defaults={"replicas": 1}),
                  vself("scaled", "Несколько реплик", "Several replicas",
                        "Реплики взаимозаменяемы и разделяют PostgreSQL, Redis, общее хранилище файлов и внешнюю векторную базу; эти службы должны быть отказоустойчивыми сами.",
                        "Replicas are interchangeable and share PostgreSQL, Redis, shared file storage and an external vector database; those services must be resilient themselves.",
                        defaults={"replicas": 2})]),
    saas("amazon_bedrock", "ai", "llm-runtime", "BDR", "Amazon Bedrock", "Amazon Bedrock",
         ("Управляемый сервис AWS с единым API к базовым моделям нескольких поставщиков. "
          "Вывод ограничен квотами на токены, заданными на модель и регион: при превышении запросы отклоняются (throttling), поэтому для пиков используют межрегиональные профили вывода (cross-Region inference) или Provisioned Throughput.",
          "An AWS managed service with a single API to foundation models from several providers. "
          "Inference is limited by token quotas set per model and Region and excess requests are throttled, so for peaks use cross-Region inference profiles or Provisioned Throughput."),
         traits=["ml"], cap=20),
    saas("azure_openai", "ai", "llm-runtime", "AOAI", "Azure OpenAI", "Azure OpenAI",
         ("Управляемый сервис Azure с API к моделям OpenAI (входит в Microsoft Foundry): развёртывания Standard, Global, Data Zone и Provisioned Throughput. "
          "Квота в токенах в минуту выдаётся на подписку, регион и модель и делится между развёртываниями, а Standard делит общий пул с другими клиентами — при нехватке ёмкости приходит 429, поэтому критичным нагрузкам берут Provisioned Throughput или два региона за шлюзом.",
          "An Azure managed service with an API to OpenAI models (part of Microsoft Foundry): Standard, Global, Data Zone and Provisioned Throughput deployments. "
          "The token-per-minute quota is granted per subscription, Region and model and split between deployments, and Standard shares a pool with other customers — when capacity runs short the API answers 429, so critical workloads take Provisioned Throughput or two regions behind a gateway."),
         traits=["ml"], cap=20),
    saas("openai_api", "ai", "llm-runtime", "OAI", "OpenAI API", "OpenAI API",
         ("Облачный API OpenAI к языковым, мультимодальным и другим моделям. "
          "Лимиты запросов и токенов в минуту задаются на организацию и проект по каждой модели и растут с уровнем использования; при превышении API отвечает 429, поэтому клиенту нужны повторы с задержкой и запасной маршрут через шлюз.",
          "The OpenAI cloud API to language, multimodal and other models. "
          "Limits on requests and tokens per minute are set per organization and project for each model and grow with the usage tier; over the limit the API answers 429, so clients need retries with backoff and a fallback route through a gateway."),
         traits=["ml"], cap=20),
    saas("anthropic_api", "ai", "llm-runtime", "CLD", "Claude API (Anthropic)", "Claude API (Anthropic)",
         ("Облачный API Claude от Anthropic (Claude Developer Platform). "
          "Лимиты запросов, входных и выходных токенов в минуту заданы на организацию по каждому классу моделей и растут с уровнем использования; при превышении приходит 429 с заголовком retry-after, поэтому клиенту нужны повторы с задержкой и запасной маршрут через шлюз или другое облако.",
          "The Claude cloud API from Anthropic (Claude Developer Platform). "
          "Limits on requests, input tokens and output tokens per minute are set per organization for each model class and grow with the usage tier; over the limit the API answers 429 with a retry-after header, so clients need retries with backoff and a fallback route through a gateway or another cloud."),
         traits=["ml"], cap=20),
    saas("yandex_ai_studio", "ai", "llm-runtime", "YAI", "Yandex AI Studio (YandexGPT)", "Yandex AI Studio (YandexGPT)",
         ("Платформа Yandex Cloud для генеративных моделей: API к YandexGPT и открытым моделям (Foundation Models) и дообучение. "
          "Сервис ведёт Yandex Cloud; запросы ограничены квотами Yandex Cloud, и превышение отклоняется, поэтому пиковую нагрузку согласуют заранее.",
          "The Yandex Cloud platform for generative models: an API to YandexGPT and open models (Foundation Models) and fine-tuning. "
          "Yandex Cloud runs the service; requests are limited by Yandex Cloud quotas and excess requests are rejected, so peak load is agreed in advance."),
         traits=["ml"], cap=10),
    saas("gigachat_api", "ai", "llm-runtime", "GGC", "GigaChat API (Сбер)", "GigaChat API (Sber)",
         ("API генеративных моделей GigaChat от Сбера для юридических и физических лиц; оплата токенами — пакетами или по факту использования. "
          "Доступность и лимиты зависят от тарифа и договора, поэтому запас на пике и обработку отказов (повторы, запасная модель) закладывают на стороне клиента.",
          "The GigaChat generative-model API from Sber for companies and individuals, paid in tokens by packages or by actual use. "
          "Availability and limits depend on the tariff and the contract, so margin for peaks and failure handling (retries, a fallback model) belong on the client side."),
         traits=["ml"], cap=10),

    # ---- Notebooks and workbenches --------------------------------------------------------------------------------------------------------
    svc("jupyterhub", "ai", "notebooks", "JHB", "JupyterHub", "JupyterHub",
        ("Многопользовательский сервер ноутбуков: Hub ведёт вход и запускает отдельный сервер Jupyter каждому пользователю через Spawner, а настраиваемый прокси направляет запросы. "
         "Hub один и режима высокой доступности не имеет; его состояние лежит в базе (SQLite по умолчанию, PostgreSQL для рабочей среды), а работа пользователей — на домашних томах, которые нужно копировать.",
         "A multi-user notebook server: the Hub handles login and starts a separate Jupyter server for each user through a Spawner, while a configurable proxy routes requests. "
         "There is a single Hub with no high-availability mode; its state lives in a database (SQLite by default, PostgreSQL for production) and users' work on home volumes, which need backups."),
        traits=PLATFORM_STATE, replicas=1, rep=("Экземпляры Hub", "Hub instances"), vcpu=2, ram=4, disk=200,
        runbook=rb(restore=[("Восстановите базу Hub (файл SQLite или PostgreSQL) и домашние тома пользователей из копий; серверы пользователей после этого запускаются заново.",
                             "Restore the Hub database (the SQLite file or PostgreSQL) and the users' home volumes from backups; user servers then start again.")]),
        variants=[vself("kubernetes", "Zero to JupyterHub (Kubernetes)", "Zero to JupyterHub (Kubernetes)",
                        "Hub и прокси — отдельные поды, серверы пользователей — поды на томах; потеря узла прерывает серверы на нём, а Hub перезапускается на другом.",
                        "The Hub and the proxy are separate pods and user servers are pods on volumes; losing a node cuts the servers on it while the Hub restarts on another.",
                        default=True, defaults={"hosting": "kubernetes"}),
                  vself("single-vm", "The Littlest JupyterHub (один сервер)", "The Littlest JupyterHub (a single server)",
                        "Дистрибутив для небольшой группы (до сотни пользователей) на одном сервере: Hub, прокси и серверы пользователей живут на нём, и его отказ останавливает всё до восстановления из копий.",
                        "A distribution for a small group (up to a hundred users) on one server: the Hub, the proxy and the user servers all live on it, so its failure stops everything until it is restored from backups.",
                        defaults={"hosting": "vm"})]),
    svc("posit_workbench", "ai", "notebooks", "PWB", "Posit Workbench (RStudio Server)", "Posit Workbench (RStudio Server)",
        ("Среда разработки для R и Python в браузере (RStudio, JupyterLab, VS Code) от Posit; открытая версия RStudio Server работает на одном узле. "
         "Несколько узлов Workbench с балансировкой требуют общего POSIX-хранилища для домашних каталогов и сессий и базы PostgreSQL; потеря узла прерывает его активные сессии.",
         "A browser-based development environment for R and Python (RStudio, JupyterLab, VS Code) from Posit; the open-source RStudio Server runs on a single node. "
         "Load-balanced Workbench nodes need shared POSIX storage for home directories and sessions and a PostgreSQL database; losing a node ends its active sessions."),
        traits=PLATFORM_STATE, replicas=1, rep=("Узлы", "Nodes"), vcpu=8, ram=32, disk=500,
        variants=[vself("single", "Один узел (RStudio Server или Workbench)", "A single node (RStudio Server or Workbench)",
                        "Все сессии на одной машине: её отказ останавливает работу всех пользователей, а домашние каталоги нужно копировать.",
                        "All sessions run on one machine: its failure stops every user, and home directories need backups.",
                        default=True, defaults={"replicas": 1, "hosting": "vm"}),
                  vself("load-balanced", "Несколько узлов с балансировкой (Workbench)", "Several load-balanced nodes (Workbench)",
                        "Узлы делят POSIX-хранилище домашних каталогов и сессий и базу PostgreSQL; балансировщик даёт единую точку входа, но хранилище и база должны быть отказоустойчивыми сами.",
                        "Nodes share POSIX storage for home directories and sessions and a PostgreSQL database; a balancer gives a single entry point, but the storage and the database must be resilient themselves.",
                        defaults={"replicas": 2, "hosting": "vm"})]),
    svc("apache_zeppelin", "ai", "notebooks", "ZPL", "Apache Zeppelin", "Apache Zeppelin",
        ("Веб-ноутбук для интерактивной аналитики с интерпретаторами Spark, Flink, Python, R, JDBC и другими. "
         "Сервер Zeppelin один: ноутбуки лежат в репозитории (по умолчанию локальный каталог, есть плагины для Git и облачных хранилищ), интерпретаторы работают отдельными процессами, а отказ сервера закрывает доступ ко всем ноутбукам до восстановления.",
         "A web notebook for interactive analytics with Spark, Flink, Python, R, JDBC and other interpreters. "
         "There is a single Zeppelin server: notebooks live in a notebook repository (a local directory by default, with plugins for Git and cloud storage) and interpreters run as separate processes, so a server failure closes access to all notebooks until it is restored."),
        traits=PLATFORM_STATE, replicas=1, rep=("Серверы Zeppelin", "Zeppelin servers"), vcpu=4, ram=16, disk=100),
    svc("code_server", "ai", "notebooks", "CDS", "code-server", "code-server",
        ("VS Code в браузере на удалённой машине (проект Coder): вся работа выполняется на сервере, клиенту нужен только браузер. "
         "Экземпляр рассчитан на одного пользователя и режима высокой доступности не имеет — для команды запускают по экземпляру на пользователя или управляют ими через Coder; рабочие файлы лежат на диске экземпляра.",
         "VS Code in the browser on a remote machine (a Coder project): all work runs on the server and the client needs only a browser. "
         "An instance is meant for one user and has no high-availability mode — for a team run one instance per user or manage them with Coder; work files live on the instance's disk."),
        traits=PLATFORM_STATE, replicas=1, rep=("Экземпляры (по одному на пользователя)", "Instances (one per user)"), vcpu=2, ram=4, disk=50),

    # ---- Feature stores -----------------------------------------------------------------------------------------------------------------
    svc("feast", "ai", "feature-store", "FEA", "Feast", "Feast",
        ("Открытый feature store: определения признаков лежат в реестре (файл в объектном хранилище или SQL-база), история — в офлайн-хранилище, свежие значения — в онлайн-хранилище (Redis, DynamoDB, PostgreSQL и др.), данные между ними переносит материализация. "
         "Сервер признаков масштабируется горизонтально и состояния не хранит: оно в реестре и хранилищах, которые защищают отдельно, а потеря онлайн-хранилища останавливает выдачу признаков.",
         "An open-source feature store: feature definitions live in a registry (a file in object storage or a SQL database), history in the offline store, fresh values in the online store (Redis, DynamoDB, PostgreSQL and others), and materialization moves data between them. "
         "The feature server scales horizontally and keeps no state: it lives in the registry and the stores, which are protected separately, and losing the online store stops feature serving."),
        traits=PLATFORM, replicas=2, rep=("Серверы признаков", "Feature servers"), vcpu=2, ram=4),
    svc("hopsworks", "ai", "feature-store", "HOPS", "Hopsworks", "Hopsworks",
        ("Платформа feature store и MLOps: онлайн-хранилище на RonDB, офлайн-хранилище в формате Delta, реестр моделей с развёртыванием на KServe и векторный индекс на OpenSearch. "
         "Для высокой доступности RonDB берут не менее двух узлов данных с двумя копиями, а брокеров запросов — не менее двух; кластер размещают в нескольких зонах доступности, но один кластер не может охватывать несколько регионов.",
         "A feature store and MLOps platform: an online store on RonDB, an offline store in Delta format, a model registry with KServe deployments and a vector index on OpenSearch. "
         "For high availability RonDB needs at least two data nodes holding two copies and at least two query brokers; a cluster can span several availability zones but not several regions."),
        traits=PLATFORM_STATE, replicas=3, rep=("Узлы кластера", "Cluster nodes"), vcpu=16, ram=64, disk=500,
        variants=[vself("self-hosted", "Свой кластер (Kubernetes)", "Own cluster (Kubernetes)",
                        "Развёртывание в управляемом Kubernetes AWS, Azure или GCP либо в изолированном ЦОД: узлы, зоны доступности и копии данных — на вас.",
                        "Deployed on managed Kubernetes on AWS, Azure or GCP or in an air-gapped data centre: nodes, availability zones and backups are up to you.", default=True),
                  vcloud("serverless", "Hopsworks Serverless (SaaS)", "Hopsworks Serverless (SaaS)", secs=None, mult=1.5)]),

    # ---- GPU and ML scheduling ------------------------------------------------------------------------------------------------------------
    svc("nvidia_gpu_operator", "ai", "ml-infra", "NGO", "NVIDIA GPU Operator", "NVIDIA GPU Operator",
        ("Оператор Kubernetes, который ставит и обновляет на GPU-узлах стек NVIDIA: драйвер, container toolkit, device plugin, DCGM Exporter, GPU Feature Discovery и MIG Manager. "
         "Оператор — контроллер в кластере, остальное работает как DaemonSet на узлах: его потеря не останавливает работающие поды, но новые GPU-узлы и обновления драйвера остаются без обслуживания, а сбой device plugin закрывает GPU узла для новых подов.",
         "A Kubernetes operator that installs and upgrades the NVIDIA stack on GPU nodes: the driver, container toolkit, device plugin, DCGM Exporter, GPU Feature Discovery and MIG Manager. "
         "The operator is a controller in the cluster and the rest runs as DaemonSets on the nodes: losing it does not stop running pods but leaves new GPU nodes and driver upgrades unserved, and a device plugin failure closes a node's GPUs to new pods."),
        traits=PLATFORM, replicas=1, rep=("Экземпляры оператора", "Operator instances"), vcpu=1, ram=1,
        runbook=rb(failover=[("kubectl get clusterpolicy показывает состояние ready; поды драйвера, device plugin и validator на GPU-узлах работают.",
                              "kubectl get clusterpolicy shows the ready state; the driver, device plugin and validator pods on the GPU nodes are running.")],
                   verify=[("У GPU-узла в allocatable есть ресурс nvidia.com/gpu (kubectl describe node), а тестовый под с запросом GPU запускается.",
                            "The GPU node lists nvidia.com/gpu in allocatable (kubectl describe node) and a test pod requesting a GPU starts.")])),
    svc("run_ai", "ai", "ml-infra", "RUN", "NVIDIA Run:ai", "NVIDIA Run:ai",
        ("Платформа оркестрации GPU для ИИ в Kubernetes: планировщик в вашем кластере делит GPU между проектами по квотам, с приоритетами и вытеснением, а управляющий слой (SaaS или свой) ведёт проекты, пользователей и запуск нагрузок. "
         "Кластер синхронизируется с ним исходящим соединением; своему управляющему слою для высокой доступности нужны несколько реплик на трёх системных узлах и внешний PostgreSQL.",
         "A GPU orchestration platform for AI on Kubernetes: the scheduler in your cluster shares GPUs between projects by quota, with priorities and preemption, while the control plane (SaaS or self-hosted) manages projects, users and workload submission. "
         "The cluster syncs to the control plane over an outbound connection; a self-hosted control plane needs several replicas on three system nodes and an external PostgreSQL to be highly available."),
        traits=PLATFORM, replicas=3, rep=("Реплики управляющего слоя", "Control plane replicas"), vcpu=8, ram=32, disk=200,
        variants=[vself("self-hosted", "Свой управляющий слой", "Self-hosted control plane",
                        "Управляющий слой работает в вашем кластере (пространство имён runai-backend) как Deployment и StatefulSet; для высокой доступности нужны несколько реплик и внешний PostgreSQL.",
                        "The control plane runs in your cluster (the runai-backend namespace) as Deployments and StatefulSets; high availability takes several replicas and an external PostgreSQL.", default=True),
                  vself("saas", "Управляющий слой как сервис (SaaS)", "Control plane as a service (SaaS)",
                        "Управляющий слой ведёт NVIDIA; планировщик остаётся в вашем кластере и синхронизируется с ним исходящим соединением.",
                        "NVIDIA runs the control plane; the scheduler stays in your cluster and syncs with it over an outbound connection.")]),
    svc("kai_scheduler", "ai", "ml-infra", "KAI", "NVIDIA KAI Scheduler", "NVIDIA KAI Scheduler",
        ("Открытый планировщик Kubernetes для ИИ-нагрузок (проект CNCF Sandbox, основан на kube-batch): оптимизирует распределение GPU в больших кластерах и работает рядом со стандартным планировщиком. "
         "Поды, назначенные этому планировщику, остаются в Pending, пока он недоступен, а запущенные поды не затрагиваются; для GPU-нагрузок нужен NVIDIA GPU Operator.",
         "An open-source Kubernetes scheduler for AI workloads (a CNCF Sandbox project built on kube-batch) that optimizes GPU allocation in large clusters and runs alongside the default scheduler. "
         "Pods that ask for it stay Pending while it is unavailable and running pods are not affected; GPU workloads need the NVIDIA GPU Operator."),
        traits=PLATFORM, replicas=1, rep=("Экземпляры планировщика", "Scheduler instances"), vcpu=2, ram=4),

    # ---- Data labeling ----------------------------------------------------------------------------------------------------------------
    svc("label_studio", "ai", "data-labeling", "LBS", "Label Studio", "Label Studio",
        ("Открытый инструмент разметки данных (тексты, изображения, аудио, видео, временные ряды) с настраиваемым интерфейсом и ML-бэкендами для предразметки. "
         "По умолчанию данные лежат в SQLite и на локальном томе — единая точка отказа; для больших проектов и многих пользователей переходят на PostgreSQL, а загруженные файлы хранят на томе или в облачном хранилище и копируют.",
         "An open-source data labeling tool (text, images, audio, video, time series) with a configurable interface and ML backends for pre-annotation. "
         "By default data sits in SQLite and on a local volume — a single point of failure; for large projects and many users move to PostgreSQL, and keep uploaded files on a volume or in cloud storage and back them up."),
        traits=PLATFORM_STATE, replicas=1, vcpu=2, ram=4, disk=100,
        variants=[vself("self-hosted", "Свой экземпляр", "Own instance",
                        "Контейнер или сервис на вашей машине: SQLite по умолчанию, PostgreSQL для рабочей среды; защита базы и файлов — на вас.",
                        "A container or a service on your machine: SQLite by default, PostgreSQL for production; protecting the database and the files is up to you.", default=True),
                  vcloud("cloud", "Облачная версия HumanSignal", "HumanSignal cloud edition", secs=None, mult=1.5)]),
    svc("cvat", "ai", "data-labeling", "CVAT", "CVAT", "CVAT",
        ("Открытый инструмент разметки для компьютерного зрения (изображения и видео): сервер и отдельные воркеры (импорт, экспорт, аннотация, фрагменты), PostgreSQL, Redis и автоматическая разметка через serverless-функции. "
         "В Docker Compose всё работает на одной машине с данными на томах, поэтому её отказ останавливает разметку; базе и файлам данных нужны копии.",
         "An open-source annotation tool for computer vision (images and video): a server and separate workers (import, export, annotation, chunks), PostgreSQL, Redis and automatic annotation through serverless functions. "
         "Docker Compose runs everything on one machine with data on volumes, so its failure stops labeling; the database and the data files need backups."),
        traits=PLATFORM_STATE, replicas=1, vcpu=4, ram=8, disk=200,
        variants=[vself("self-hosted", "Свой экземпляр", "Own instance",
                        "Docker Compose на одной машине или Helm-чарт в Kubernetes; база, Redis и тома данных — ваша забота.",
                        "Docker Compose on one machine or a Helm chart on Kubernetes; the database, Redis and the data volumes are your concern.", default=True),
                  vcloud("cloud", "CVAT Online", "CVAT Online", secs=None, mult=1.5)]),
]
